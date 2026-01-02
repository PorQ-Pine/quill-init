/*
 * quill-init: Initialization program of Quill OS
 * Copyright (C) 2025-2026 Nicolas Mailloux <nicolecrivain@gmail.com>
 * SPDX-License-Identifier: GPL-3.0-only
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

cfg_if::cfg_if! {
    if #[cfg(feature = "init_wrapper")] {
        use libqinit::OVERLAY_MOUNTPOINT;
        use exec;
        use std::process::Command;
        use core::ops::Deref;
        use std::os::unix;
        use std::thread;
        use signal_hook::{iterator::Signals, consts::signal::*};
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
        use nix::unistd::Pid;
        use libqinit::eink::ScreenRotation;
        use libqinit::system::{mount_base_filesystems, mount_base_partitions};

        pub const QINIT_PATH: &str = "/etc/init.d/qinit";
    } else {
        cfg_if::cfg_if! {
            if #[cfg(not(feature = "gui_only"))] {
                use libqinit::eink;
                use libqinit::system::{mount_modules, mount_firmware, set_workdir, run_command, set_timezone};
                use libqinit::rootfs;
                use libqinit::systemd;

                use nix::unistd::sethostname;
                use crossterm::event::{self, Event};

                #[cfg(feature = "debug")]
                mod debug;
            }
        }
        mod gui;

        use libqinit::signing::{read_public_key};
        use libqinit::system::{generate_version_string, generate_short_version_string, shut_down, BootCommand, BootCommandForm};
        use libqinit::rootfs_socket;
        use std::time::Duration;
        use std::thread;
        use std::sync::{Arc, atomic::AtomicBool, Mutex};

        const SYSTEMD_NO_TARGETS: i32 = -1;
        const QINIT_SOCKET: &str = "qinit.sock";
    }
}

use anyhow::{Context, Result};
use libqinit::boot_config::BootConfig;
use libquillcom::socket;
use log::{error, info};
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::mpsc::{Receiver, Sender, channel};
pub const QINIT_LOG_DIR: &str = "/var/log";
pub const QINIT_LOG_FILE: &str = "qinit.log";
pub const MAX_COPYRIGHT_YEAR: &str = env!("BUILD_YEAR");
const BOOT_SOCKET_PATH: &str = "/qinit.sock";

#[derive(Serialize, Deserialize)]
struct OverlayStatus {
    ready: bool,
}

fn main() {
    env_logger::init();
    let (interrupt_sender, interrupt_receiver): (Sender<String>, Receiver<String>) = channel();
    let interrupt_sender_clone = interrupt_sender.clone();
    if let Err(e) = init(interrupt_sender_clone, interrupt_receiver) {
        let mut error_string = format!("Reason: {}\nCaused by: ", &e);
        let error_string_initial_length = error_string.len();
        e.chain()
            .skip(1)
            .for_each(|cause| error_string.push_str(&cause.to_string()));
        if error_string_initial_length == error_string.chars().count() {
            error_string.truncate(error_string_initial_length - 12);
        }
        error!("{}", &error_string.replace("\n", " | "));
        // Send error reason to GUI (if ever it is alive)
        let _ = interrupt_sender.send(error_string);
    }

    cfg_if::cfg_if! {
        if #[cfg(not(feature = "init_wrapper"))] {
            thread::park();
        }
    }
}

fn init(interrupt_sender: Sender<String>, interrupt_receiver: Receiver<String>) -> Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(feature = "init_wrapper")] {
            first_stage_info("qinit binary starting");

            // Install signal handler for SIGCHLD (i.e. allow us to stop iwd after having started it, for example)
            let mut signals = Signals::new(&[SIGCHLD])?;
            thread::spawn(move || {
                for _sig in signals.forever() {
                    reap_zombies();
                }
            });

            let boot_unix_listener = socket::bind(&BOOT_SOCKET_PATH)?;

            mount_base_filesystems()?;
            mount_base_partitions()?;
            let rotation_env_var_base = "SLINT_KMS_ROTATION=";
            let rotation_env_var;
            let (boot_config, _) = BootConfig::read()?;
            if boot_config.system.initial_screen_rotation == ScreenRotation::Cw0 {
                rotation_env_var = format!("{}0", &rotation_env_var_base);
            } else if boot_config.system.initial_screen_rotation == ScreenRotation::Cw90 {
                rotation_env_var = format!("{}90", &rotation_env_var_base);
            } else if boot_config.system.initial_screen_rotation == ScreenRotation::Cw180 {
                rotation_env_var = format!("{}180", &rotation_env_var_base);
            } else {
                rotation_env_var = format!("{}270", &rotation_env_var_base);
            }
            first_stage_info(&format!("Initial screen rotation is {:?}", &boot_config.system.initial_screen_rotation));

            first_stage_info("Spawning second stage qinit binary");
            fs::create_dir_all(&QINIT_LOG_DIR)?;
            Command::new("/bin/sh").args(&["-c", &format!("env {} {} 2>&1 | tee -a {}", &rotation_env_var, &QINIT_PATH, &format!("{}/{}", &QINIT_LOG_DIR, &QINIT_LOG_FILE))]).spawn().with_context(|| "Failed to spawn second stage qinit binary")?;

            first_stage_info("Waiting for status message from second stage qinit binary");
            let status = from_bytes::<OverlayStatus>(socket::read(&boot_unix_listener)?.deref())?;

            if status.ready {
                first_stage_info("Ready for systemd initialization");
                fs::remove_file(&BOOT_SOCKET_PATH).with_context(|| "Failed to remove qinit UNIX socket file")?;

                first_stage_info("Entering rootfs chroot, goodbye");
                unix::fs::chroot(&OVERLAY_MOUNTPOINT).with_context(|| "Failed to chroot to overlay filesytem's mountpoint")?;
                std::env::set_current_dir("/").with_context(|| "Failed to set current directory to / (chroot)")?;
                let _ = exec::Command::new("/sbin/init").exec();
            }
        } else {
            // System initialization
            info!("(Second stage) qinit binary starting");
            #[cfg(not(feature = "gui_only"))]
            {
                sethostname("pinenote").with_context(|| "Failed to set device's hostname")?;
                run_command("/sbin/ifconfig", &["lo", "up"]).with_context(|| "Failed to set loopback network device up")?;
            }

            // Boot info
            let mut kernel_version = fs::read_to_string("/proc/version").with_context(|| "Failed to read kernel version")?; kernel_version.pop();
            let mut kernel_commit = fs::read_to_string("/.commit").with_context(|| "Failed to read kernel commit")?; kernel_commit.pop();

            let pubkey = read_public_key()?;

            #[cfg(not(feature = "gui_only"))]
            {
                set_workdir("/").with_context(|| "Failed to set current directory to / (not in chroot)")?;
                fs::create_dir_all(&libqinit::DEFAULT_MOUNTPOINT).with_context(|| "Failed to create default mountpoint's directory")?;

                mount_modules()?;
                let _ = mount_firmware(&pubkey);
            }

            // Read boot configuration
            let (original_boot_config, boot_config_valid) = BootConfig::read()?;
            info!("Original boot configuration: {:?}", &original_boot_config);
            let mut boot_config = original_boot_config.clone();

            // Version strings
            let version_string = generate_version_string(&mut boot_config, &git_const::git_hash!()[0..12], &kernel_commit);
            let short_version_string = generate_short_version_string(&kernel_commit, &kernel_version);

            #[cfg(not(feature = "gui_only"))]
            {
                #[cfg(feature = "debug")]
                if let Err(e) = debug::start_debug_framework(&pubkey, &mut boot_config) {
                    error!("Failed to initialize debug framework: {}", &e);
                }

                eink::load_waveform()?;
                eink::load_modules()?;
                eink::setup_touchscreen(&mut boot_config)?;

                set_timezone(&boot_config.system.timezone)?;

                println!("{}\n\nQuill OS, kernel commit {}\nCopyright (C) 2021-{} Nicolas Mailloux <nicolecrivain@gmail.com> and Szybet <https://github.com/Szybet>\n", &kernel_version, &kernel_commit, &MAX_COPYRIGHT_YEAR);
                print!("(initrd) Hit any key to stop auto-boot ... ");

                // Flush stdout to ensure prompt is shown before waiting
                std::io::Write::flush(&mut std::io::stdout()).unwrap();

                if event::poll(Duration::from_millis(500)).unwrap() {
                    if let Event::Key(_) = event::read().unwrap() {
                        loop {
                            let _ = run_command("/sbin/getty", &["-L", "ttyS2", "1500000", "linux"]);
                        }
                    }
                }
                println!();
            }

            // Setup GUI
            let mut systemd_targets_total = SYSTEMD_NO_TARGETS;
            #[cfg(not(feature = "gui_only"))]
            {
                if let Some(targets_total) = systemd::get_targets_total(&mut boot_config)? {
                    systemd_targets_total = targets_total;
                }
            }
            let display_progress_bar = systemd_targets_total != SYSTEMD_NO_TARGETS;
            let (progress_sender, progress_receiver): (Sender<f32>, Receiver<f32>) = channel();
            let (boot_sender, boot_receiver): (Sender<BootCommandForm>, Receiver<BootCommandForm>) = channel();
            let (toast_sender, toast_receiver): (Sender<String>, Receiver<String>) = channel();
            let (login_credentials_sender, login_credentials_receiver): (Sender<socket::LoginForm>, Receiver<socket::LoginForm>) = channel();
            let (splash_sender, splash_receiver): (Sender<socket::PrimitiveShutDownType>, Receiver<socket::PrimitiveShutDownType>) = channel();
            let (splash_ready_sender, splash_ready_receiver): (Sender<()>, Receiver<()>) = channel();

            let boot_config_mutex = Arc::new(Mutex::new(boot_config.clone()));
            thread::spawn({
                let boot_config_mutex = boot_config_mutex.clone();
                let toast_sender = toast_sender.clone();
                move || {
                    gui::setup_gui(progress_receiver, boot_sender, login_credentials_sender, splash_receiver, splash_ready_sender, interrupt_receiver, toast_sender, toast_receiver, version_string, short_version_string, display_progress_bar, boot_config_mutex, boot_config_valid)
                }
            });

            // Block this function until the main thread receives a signal to continue booting (allowing a user to perform recovery tasks, for example)
            let boot_command_form  = boot_receiver.recv()?;
            let (mut boot_command, can_shut_down) = handle_boot_command(boot_command_form);

            boot_config = boot_config_mutex.lock().unwrap().clone();
            info!("Boot configuration after possible modifications: {:?}", &boot_config);

            // Check if we need to force a reboot here to apply configuration changes
            let mut config_force_reboot = true;
            if boot_config.rootfs.persistent_storage != original_boot_config.rootfs.persistent_storage {
                // It might be useful to recount the number of systemd targets
                boot_config.rootfs.systemd_targets_total = None;
            } else {
                config_force_reboot = false;
            }

            if config_force_reboot {
                if boot_command == BootCommand::NormalBoot {
                    boot_command = BootCommand::Reboot;
                    toast_sender.send("Applying changes".to_string())?;
                    BootConfig::write(&mut boot_config, false)?;
                    std::thread::sleep(Duration::from_millis(gui::TOAST_DURATION_MILLIS as u64));

                    shut_down(libquillcom::socket::PrimitiveShutDownType::Reboot, libqinit::system::PowerDownMode::Normal, Arc::new(AtomicBool::new(true)))?;
                }
            } else {
                // Trigger switch to boot splash page
                if boot_command == BootCommand::NormalBoot {
                    progress_sender.send(0.0)?;
                }
            }

            if boot_command != BootCommand::NormalBoot {
                if !boot_config_valid || boot_config != original_boot_config {
                    BootConfig::write(&mut boot_config, false)?;
                } else {
                    info!("Boot configuration did not change: not writing it back");
                }

                match boot_command {
                    BootCommand::PowerOff => {
                        shut_down(libquillcom::socket::PrimitiveShutDownType::PowerOff, libqinit::system::PowerDownMode::Normal, can_shut_down)?;
                        return Ok(());
                    },
                    BootCommand::Reboot => {
                        shut_down(libquillcom::socket::PrimitiveShutDownType::Reboot, libqinit::system::PowerDownMode::Normal, can_shut_down)?;
                        return Ok(());
                    },
                    _ => {},
                };
            }

            // Function that will always fail: can be used for debugging error splash GUI
            // fs::read("/aaa/bbb").with_context(|| "Dummy error")?;

            #[cfg(not(feature = "gui_only"))]
            {
                // Resume boot
                rootfs::setup(&pubkey, boot_config.rootfs.persistent_storage)?;
            }

            // Socket used for binaries inside the chroot wishing to invoke a 'Fatal error' splash
            let qinit_socket_path = format!("{}/run/{}", &libqinit::OVERLAY_MOUNTPOINT, &QINIT_SOCKET);
            std::thread::spawn(move || {
                if let Ok(qinit_unix_listener) = socket::bind(&qinit_socket_path) {
                    // This is a one-time call: any more fatal errors are useless since we already block the UI until the next boot
                    if let Ok(qinit_unix_listener_socket) = socket::read(&qinit_unix_listener) {
                        info!("Received request to show fatal error splash: proceeding");
                        if let Ok(error_details) = from_bytes::<socket::ErrorDetails>(&qinit_unix_listener_socket) {
                            let _ = interrupt_sender.send(error_details.error_reason);
                            let _ = fs::remove_file(&qinit_socket_path);
                        }
                    }
                }
            });

            #[cfg(not(feature = "gui_only"))] {
                let overlay_status = to_allocvec(&OverlayStatus { ready: true }).with_context(|| "Failed to create vector with boot command")?;
                let _ = socket::write(&BOOT_SOCKET_PATH, &overlay_status)?;

                thread::spawn(move || rootfs_socket::initialize(login_credentials_receiver, splash_sender, splash_ready_receiver, can_shut_down.clone()));

                if display_progress_bar {
                    progress_sender.send(rootfs::ROOTFS_MOUNTED_PROGRESS_VALUE)?;
                    systemd::wait_for_targets(&mut boot_config, systemd_targets_total, progress_sender)?;
                } else {
                    // Only runs on first boot or when boot configuration is cleared/corrupted
                    systemd::wait_and_count_targets(Some(&mut boot_config), Some(progress_sender), None)?;
                }

                // Wait until systemd startup has completed
                let boot_command_form = boot_receiver.recv()?;
                let (boot_command, can_shut_down) = handle_boot_command(boot_command_form);
                info!("systemd startup complete");
                if !boot_config_valid || boot_config != original_boot_config {
                    BootConfig::write(&mut boot_config, false)?;
                }

                match boot_command {
                    BootCommand::PowerOffRootFS => {
                        shut_down(libquillcom::socket::PrimitiveShutDownType::PowerOff, libqinit::system::PowerDownMode::RootFS, can_shut_down)?;
                        return Ok(());
                    },
                    BootCommand::RebootRootFS => {
                        shut_down(libquillcom::socket::PrimitiveShutDownType::Reboot, libqinit::system::PowerDownMode::RootFS, can_shut_down)?;
                        return Ok(());
                    }
                    BootCommand::BootFinished | _ => {}
                }
            }
        }
    }

    Ok(())
}

#[cfg(not(feature = "init_wrapper"))]
fn handle_boot_command(boot_command_form: BootCommandForm) -> (BootCommand, Arc<AtomicBool>) {
    return (
        boot_command_form.command,
        boot_command_form
            .can_shut_down
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
    );
}

cfg_if::cfg_if! {
    if #[cfg(feature = "init_wrapper")] {
        fn first_stage_info(message: &str) {
            info!("(First stage) {}", &message);
        }
        fn first_stage_error(message: &str) {
            error!("(First stage) {}", &message);
        }

        // Thanks, ChatGPT
        fn reap_zombies() {
            loop {
                match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::Exited(pid, status)) => {
                        first_stage_info(&format!("Child {} exited with status {}", pid, status));
                    }
                    Ok(WaitStatus::Signaled(pid, sig, _)) => {
                        first_stage_info(&format!("Child {} killed by signal {:?}", pid, sig));
                    }
                    Ok(WaitStatus::StillAlive) => {
                        break;
                    }
                    Ok(_) => {} // Other wait statuses
                    Err(nix::Error::ECHILD) => {
                        break; // No more children
                    }
                    Err(e) => {
                        first_stage_error(&format!("waitpid error: {:?}", e));
                        break;
                    }
                }
            }
        }
    }
}
