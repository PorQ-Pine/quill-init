/*
 * quill-init: Initialization program of Quill OS
 * Copyright (C) 2025 Nicolas Mailloux <nicolecrivain@gmail.com>
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
        use fork::{daemon, Fork};
        use std::process::Command;
        use std::io::Read;
        use postcard::{from_bytes};
        use core::ops::Deref;
        use std::os::unix;
        const QINIT_PATH: &str = "/etc/init.d/qinit";
    } else {
        mod eink;
        mod gui;
        #[cfg(feature = "debug")]
        mod debug;
        
        use crossterm::event::{self, Event};
        use std::time::Duration;
        use libqinit::system::{mount_base_filesystems, mount_data_partition, mount_firmware, set_workdir, generate_version_string, run_command};
        use libqinit::rootfs;
        use libqinit::signing::{read_public_key};
        use std::sync::mpsc::{channel, Sender, Receiver};
        use nix::unistd::sethostname;
        use postcard::{to_allocvec};
        use libqinit::flag;
    }
}

use anyhow::{Context, Result};
use log::{info, warn, error};
use std::{thread};
use std::fs;
use serde::{Serialize, Deserialize};
use libqinit::socket;
const QINIT_SOCKET_PATH: &str = "/qinit.sock";

#[derive(Serialize, Deserialize)]
struct OverlayStatus {
    ready: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    cfg_if::cfg_if! {
        if #[cfg(feature = "init_wrapper")] {
            first_stage_info("qinit binary starting");
            let unix_listener = socket::bind(&QINIT_SOCKET_PATH)?;
            
            first_stage_info("Spawning second-stage qinit binary");
            Command::new(&QINIT_PATH).spawn().with_context(|| "Failed to spawn second-stage qinit binary")?;

            first_stage_info("Waiting for status message from second stage qinit binary");
            let status = from_bytes::<OverlayStatus>(socket::read(unix_listener)?.deref())?;
            if status.ready {
                first_stage_info("Ready for systemd initialization");
                fs::remove_file(&QINIT_SOCKET_PATH)?;

                first_stage_info("Entering rootfs chroot, goodbye");
                unix::fs::chroot(&OVERLAY_MOUNTPOINT)?;
                std::env::set_current_dir("/")?;
                let _ = exec::Command::new("/sbin/init").exec();
            }
        } else {
            // System initialization
            info!("(Second stage) qinit binary starting");
            #[cfg(not(feature = "gui_only"))]
            {
                mount_base_filesystems()?;
                sethostname("pinenote")?;
                run_command("/sbin/ifconfig", &["lo", "up"])?;
            }

            // Boot info
            let mut kernel_version = fs::read_to_string("/proc/version").with_context(|| "Failed to read kernel version")?; kernel_version.pop();
            let mut kernel_commit = fs::read_to_string("/.commit").with_context(|| "Failed to read kernel commit")?; kernel_commit.pop();
            let version_string = generate_version_string(&kernel_commit);

            // Decode public key embedded in kernel command line
            let pubkey = read_public_key()?;

            #[cfg(not(feature = "gui_only"))]
            {
                set_workdir("/")?;
                fs::create_dir_all(&libqinit::DEFAULT_MOUNTPOINT)?;

                mount_data_partition()?;
                mount_firmware(&pubkey)?;

                // Create boot flags directory
                flag::create_flags_dir()?;

                #[cfg(feature = "debug")]
                debug::start_debug_framework(&pubkey)?;

                eink::load_waveform()?;
                eink::load_modules()?;
                eink::setup_touchscreen()?;

                println!("{}\n\nQuill OS, kernel commit {}\nCopyright (C) 2021-2025 Nicolas Mailloux <nicolecrivain@gmail.com> and Szybet <https://github.com/Szybet>\n", &kernel_version, &kernel_commit);
                print!("(initrd) Hit any key to stop auto-boot ... ");

                // Flush stdout to ensure prompt is shown before waiting
                std::io::Write::flush(&mut std::io::stdout()).unwrap();

                if event::poll(Duration::from_secs(3)).unwrap() {
                    if let Event::Key(_) = event::read().unwrap() {
                        loop {
                            let _ = run_command("/sbin/getty", &["-L", "ttyS2", "1500000", "linux"]);
                        }
                    }
                }
                println!();
            }

            // Setting up GUI
            let (progress_sender, progress_receiver): (Sender<f32>, Receiver<f32>) = channel();
            let (init_boot_sender, init_boot_receiver): (Sender<bool>, Receiver<bool>) = channel();
            let gui_handle = thread::spawn(move || gui::setup_gui(progress_receiver, init_boot_sender, &version_string));

            // Blocking this function until the main thread receives a signal to continue booting (allowing an user to perform recovery tasks, for example)
            init_boot_receiver.recv()?;

            // Resuming boot
            #[cfg(not(feature = "gui_only"))]
            {
                rootfs::setup(&pubkey)?;
                let overlay_status = to_allocvec(&OverlayStatus { ready: true })?;
                socket::write(&QINIT_SOCKET_PATH, &overlay_status)?;
                progress_sender.send(0.1)?;
            }

            // Handling GUI thread issues if there are some
            gui_handle.join().map_err(|e| anyhow::anyhow!("Thread panicked: {:?}", e))??;
        }
    }

    Ok(())
}

#[cfg(feature = "init_wrapper")]
fn first_stage_info(message: &str) {
    info!("(First stage) {}", &message);
}
