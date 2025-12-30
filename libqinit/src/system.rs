use anyhow::{Context, Result};
use base64::prelude::*;
use libquillcom::socket::PrimitiveShutDownType;
use log::{debug, info, warn};
use openssl::pkey::PKey;
use openssl::pkey::Public;
use rand::Rng;
use rand::distr::Alphanumeric;
use regex::Regex;
use rmesg;
use sha256;
use std::env;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{fs, process::Command, thread, time::Duration};
use sys_mount::{Mount, UnmountFlags, unmount};

use crate::boot_config::BootConfig;
use crate::rootfs::run_chroot_command;
use crate::signing::check_signature;

pub const MODULES_DIR_PATH: &str = "/lib/modules";
pub const MODULES_ARCHIVE: &str = "modules.squashfs";
pub const FIRMWARE_DIR_PATH: &str = "/lib/firmware";
pub const FIRMWARE_ARCHIVE: &str = "firmware.squashfs";
pub const WAVEFORM_DIR_PATH: &str = "/lib/firmware/rockchip/";
pub const QINIT_BINARIES_ARCHIVE: &str = "qinit_binaries.squashfs";
pub const QINIT_BINARIES_DIR_PATH: &str = "/qinit_binaries/";

const REBOOT_BINARY_PATH: &str = "/sbin/reboot";
const POWER_OFF_BINARY_PATH: &str = "/sbin/poweroff";

#[derive(PartialEq)]
pub enum BootCommand {
    PowerOff,
    PowerOffRootFS,
    Reboot,
    RebootRootFS,
    NormalBoot,
    BootFinished,
}

pub struct BootCommandForm {
    pub command: BootCommand,
    pub can_shut_down: Option<Arc<AtomicBool>>,
}

#[derive(PartialEq)]
pub enum PowerDownMode {
    Normal,
    RootFS,
}

pub fn mount_base_filesystems() -> Result<()> {
    Mount::builder()
        .fstype("proc")
        .mount("proc", "/proc")
        .with_context(|| "Failed to mount proc base filesystem")?;
    Mount::builder()
        .fstype("sysfs")
        .mount("sysfs", "/sys")
        .with_context(|| "Failed to mount base sysfs")?;
    Mount::builder()
        .fstype("devtmpfs")
        .mount("devtmpfs", "/dev")
        .with_context(|| "Failed to mount base devtmpfs")?;
    fs::create_dir_all("/dev/pts")?;
    Mount::builder()
        .fstype("devpts")
        .mount("devpts", "/dev/pts")
        .with_context(|| "Failed to mount devpts base filesystem")?;
    Mount::builder()
        .fstype("tmpfs")
        .mount("tmpfs", "/tmp")
        .with_context(|| "Failed to mount base tmpfs ('/tmp')")?;
    Mount::builder()
        .fstype("tmpfs")
        .mount("tmpfs", "/run")
        .with_context(|| "Failed to mount base tmpfs ('/run')")?;

    Ok(())
}

pub fn get_cmdline_bool(property: &str) -> Result<bool> {
    info!(
        "Trying to extract boolean value for property '{}' in kernel command line",
        &property
    );
    let cmdline = fs::read_to_string("/proc/cmdline")?;
    let re_str = format!(r"{}=(\w+)", regex::escape(&property));
    let re = Regex::new(&re_str)?;
    if let Some(captures) = re.captures(&cmdline) {
        if let Some(value_match) = captures.get(1) {
            let value = value_match.as_str();
            if value == "1" || value == "true" {
                info!("Property '{}' is true", &property);
                return Ok(true);
            } else {
                info!("Property '{}' is false", &property);
                return Ok(false);
            }
        } else {
            info!("Error getting capture group: returning false");
            return Ok(false);
        }
    } else {
        info!("Could not find property: returning false");
        return Ok(false);
    }
}

pub fn set_workdir(path: &str) -> Result<()> {
    let root = Path::new(path);
    env::set_current_dir(&root)?;

    Ok(())
}

pub fn wait_for_path(path: &str) -> Result<()> {
    while !fs::exists(&path)? {
        thread::sleep(Duration::from_millis(100));
    }

    Ok(())
}

pub fn run_command(command: &str, args: &[&str]) -> Result<()> {
    debug!(
        "Running command '{}' with arguments '{}'",
        &command,
        &args.join(" ")
    );
    let status = Command::new(&command)
        .args(args)
        .status()
        .with_context(|| format!("Failed to execute command: {}", &command))?;

    debug!("Exit status is {}", &status);
    if status.success() {
        Ok(())
    } else {
        return Err(anyhow::anyhow!(
            "Command `{}` exited with status: {}",
            &command,
            &status
        ));
    }
}

pub fn modprobe(args: &[&str]) -> Result<()> {
    run_command("/sbin/modprobe", &args)
        .with_context(|| format!("Failed to load module; modprobe arguments: {:?}\n", &args))?;

    Ok(())
}

pub fn mount_base_partitions() -> Result<()> {
    info!("Mounting boot partition");
    fs::create_dir_all(&crate::BOOT_PART_MOUNTPOINT)
        .with_context(|| "Failed to create boot partition mountpoint's directory")?;
    wait_for_path(&crate::BOOT_PART)?;
    Mount::builder()
        .fstype("ext4")
        .data("rw")
        .mount(&crate::BOOT_PART, &crate::BOOT_PART_MOUNTPOINT)
        .with_context(|| "Failed to mount boot partition")?;

    info!("Mounting main partition");
    fs::create_dir_all(&crate::MAIN_PART_MOUNTPOINT)
        .with_context(|| "Failed to create boot partition mountpoint's directory")?;
    wait_for_path(&crate::MAIN_PART)?;
    Mount::builder()
        .fstype("ext4")
        .data("rw")
        .mount(&crate::MAIN_PART, &crate::MAIN_PART_MOUNTPOINT)
        .with_context(|| "Failed to mount main partition")?;

    fs::create_dir_all(&format!(
        "{}/{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_DIR
    ))?;
    fs::create_dir_all(&format!(
        "{}/{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_HOME_DIR
    ))?;

    Ok(())
}

pub fn mount_modules() -> Result<()> {
    info!("Mounting kernel modules SquashFS archive");

    fs::create_dir_all(&MODULES_DIR_PATH)?;
    let modules_archive_path = format!("/lib/{}", &MODULES_ARCHIVE);

    run_command("/bin/mount", &[&modules_archive_path, &MODULES_DIR_PATH])
        .with_context(|| "Failed to mount kernel modules archive")?;

    Ok(())
}

pub fn mount_firmware(pubkey: &PKey<Public>) -> Result<()> {
    info!("Mounting system firmware SquashFS archive");
    let firmware_archive_path = format!("{}/{}", &crate::BOOT_PART_MOUNTPOINT, &FIRMWARE_ARCHIVE);
    if fs::exists(&firmware_archive_path)? && check_signature(&pubkey, &firmware_archive_path)? {
        // musl introduces compile-time issues with the 'loop' feature of the 'sys_mount' crate: I have disabled it. Thus, here we need to use an external binary to mount SquashFS files.
        run_command("/bin/mount", &[&firmware_archive_path, &FIRMWARE_DIR_PATH])
            .with_context(|| "Failed to mount device's firmware")?;
        Mount::builder()
            .fstype("tmpfs")
            .data("size=32M")
            .mount("tmpfs", &WAVEFORM_DIR_PATH)
            .with_context(|| "Failed to mount eInk firmware's tmpfs")?;
    } else {
        return Err(anyhow::anyhow!(
            "Either system firmware SquashFS archive was not found, either its signature was invalid"
        ));
    }

    Ok(())
}

pub fn unmount_base_partitions() -> Result<()> {
    sync_disks()?;
    info!("Unmounting main partition");
    bulletproof_unmount(&crate::MAIN_PART_MOUNTPOINT)?;
    info!("Unmounting data partition");
    bulletproof_unmount(&crate::BOOT_PART_MOUNTPOINT)?;

    Ok(())
}

pub fn sync_disks() -> Result<()> {
    info!("Syncing disks");
    run_command("/bin/sync", &[])?;

    Ok(())
}

pub fn start_service(service: &str) -> Result<()> {
    run_command("/sbin/rc-service", &[&service, "start"])
        .with_context(|| format!("Failed to start '{}' service", &service))?;

    Ok(())
}

pub fn stop_service(service: &str) -> Result<()> {
    run_command("/sbin/rc-service", &[&service, "stop"])
        .with_context(|| format!("Failed to stop '{}' service", &service))?;

    Ok(())
}

pub fn restart_service(service: &str) -> Result<()> {
    run_command("/sbin/rc-service", &[&service, "restart"])
        .with_context(|| format!("Failed to restart '{}' service", &service))?;

    Ok(())
}

pub fn real_shut_down(shut_down_type: PrimitiveShutDownType, mode: PowerDownMode) -> Result<()> {
    match shut_down_type {
        PrimitiveShutDownType::PowerOff => warn!("Powering off"),
        PrimitiveShutDownType::Reboot => warn!("Rebooting"),
        _ => {}
    };

    cfg_if::cfg_if! {
        if #[cfg(not(feature = "gui_only"))] {
            match mode {
                PowerDownMode::Normal => {
                    unmount_base_partitions()?;
                    match shut_down_type {
                        PrimitiveShutDownType::PowerOff => run_command(&POWER_OFF_BINARY_PATH, &["-f"])?,
                        PrimitiveShutDownType::Reboot => run_command(&REBOOT_BINARY_PATH, &["-f"])?,
                        _ => {},
                    }
                },
                PowerDownMode::RootFS => {
                    match shut_down_type {
                        PrimitiveShutDownType::PowerOff => run_chroot_command(&[&POWER_OFF_BINARY_PATH])?,
                        PrimitiveShutDownType::Reboot => run_chroot_command(&[&REBOOT_BINARY_PATH])?,
                        _ => {},
                    }
                }
            }
        }
    }

    Ok(())
}

pub fn shut_down(
    shut_down_type: PrimitiveShutDownType,
    mode: PowerDownMode,
    can_shut_down: Arc<AtomicBool>,
) -> Result<()> {
    loop {
        if can_shut_down.load(Ordering::SeqCst) {
            can_shut_down.store(false, Ordering::SeqCst);
            break;
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }

    thread::spawn(move || real_shut_down(shut_down_type, mode));

    Ok(())
}

pub fn generate_version_string(
    boot_config: &mut BootConfig,
    qinit_commit: &str,
    kernel_commit: &str,
) -> String {
    cfg_if::cfg_if! {
        if #[cfg(feature = "free_roam")] {
            let signing_state = "Package signing protection: disabled";
        } else {
            let signing_state = "Package signing protection: enabled";
        }
    }
    cfg_if::cfg_if! {
        if #[cfg(feature = "debug")] {
            let debug_state = "Debug mode: enabled";
        } else {
            let debug_state = "Debug mode: disabled";
        }
    }

    let recovery_features_state;
    if boot_config.system.recovery_features {
        recovery_features_state = "Recovery features: enabled";
    } else {
        recovery_features_state = "Recovery features: disabled";
    }

    let version_string = format!(
        "Kernel commit: {}\nGUI commit: {}\n{}\n{}\n{}",
        &kernel_commit, &qinit_commit, &recovery_features_state, &signing_state, &debug_state
    );

    return version_string;
}

pub fn generate_short_version_string(kernel_commit: &str, kernel_version: &str) -> String {
    format!(
        "Quill OS, kernel commit {}\n{}",
        &kernel_commit, &kernel_version
    )
}

pub fn bind_mount(source: &str, mountpoint: &str) -> Result<()> {
    // Please figure out why Mount::builder() does not work for this kind of mount
    run_command("mount", &["--rbind", &source, &mountpoint])?;

    Ok(())
}

pub fn clean_copy_dir_recursively(source: &str, target: &str) -> Result<()> {
    info!(
        "Recursively copying directory '{}' to '{}'",
        &source, &target
    );
    rm_dir_all(&target)?;
    run_command("/bin/cp", &["-r", &source, &target])?;
    // This does not seem to work with /overlay/etc/ssh directory (permission issues?)
    /* fs::create_dir_all(&target)?;
    let mut path = Vec::new();
    path.push(&source);
    copy_items(&path, &target, &dir::CopyOptions::new())?; */

    Ok(())
}

pub fn sha256_match(path: &str, write_new_checksum: bool) -> Result<bool> {
    let checksum = sha256::try_digest(Path::new(&path))?;
    let checksum_file_path = format!("{}.sha256", &path);
    info!(
        "Checking for sha256sum match for file '{}' at path '{}'",
        &path, &checksum_file_path
    );
    if fs::exists(&checksum_file_path)?
        && fs::read_to_string(&checksum_file_path)?.trim() == checksum
    {
        info!("Checksum matches");
        return Ok(true);
    } else {
        warn!("Checksum does not match");
        if write_new_checksum {
            info!("Writing new checksum calculated with current file");
            fs::write(&checksum_file_path, &checksum)?;
        }
        return Ok(false);
    }
}

pub fn read_kernel_buffer_singleshot() -> Result<String> {
    info!("Reading kernel buffer");
    let mut kernel_buffer = String::new();
    let entries = rmesg::log_entries(rmesg::Backend::Default, false)
        .with_context(|| "Failed to read kernel buffer")?;
    for entry in entries {
        kernel_buffer.push_str(&entry.to_string());
        kernel_buffer.push_str("\n");
    }
    // Remove extraneous newline at the end
    kernel_buffer.truncate(kernel_buffer.len() - 1);

    Ok(kernel_buffer)
}

pub fn keep_last_lines(string: &str, lines_to_keep: usize) -> String {
    let lines: Vec<&str> = string.lines().collect();
    let len = lines.len();
    return lines
        .into_iter()
        .skip(len.saturating_sub(lines_to_keep))
        .collect::<Vec<_>>()
        .join("\n");
}

pub fn compress_string_to_xz(string: &str) -> Result<Vec<u8>> {
    debug!("Compressing string to xz");
    let base64_string = BASE64_STANDARD.encode(string);
    let data = Command::new("/bin/sh")
        .args(&[
            "-c",
            &format!("printf '{}' | base64 -d | xz -9 -e", &base64_string),
        ])
        .output()?
        .stdout;
    debug!("Compressed string size: {} bytes", data.iter().count());

    Ok(data)
}

pub fn sync_time() -> Result<()> {
    // This function assumes a working Internet connection
    info!("Syncing time");
    run_command("/bin/busybox", &["ntpd", "-q", "-n", "-p", "pool.ntp.org"])?;
    run_command("/sbin/hwclock", &["--systohc"])?;

    Ok(())
}

pub fn set_timezone(timezone: &str) -> Result<()> {
    let timezone_data = format!("/usr/share/zoneinfo/{}", &timezone);
    if fs::exists(&&timezone_data)? {
        symlink(&timezone_data, "/etc/localtime")
            .with_context(|| "Failed to symlink timezone data to /etc/localtime")?;
        info!("Setting timezone to '{}'", &timezone);
    } else {
        info!("Setting timezone to 'UTC'")
    }
    // If nothing is symlinked, the OS will just default to the UTC timezone

    Ok(())
}

// https://docs.rs/rand/latest/rand/distr/struct.Alphanumeric.html
pub fn generate_random_string(length: i32) -> Result<String> {
    let mut rng = rand::rng();
    let chars: String = (0..length)
        .map(|_| rng.sample(Alphanumeric) as char)
        .collect();

    Ok(chars)
}

pub fn rm_dir_all(path: &str) -> Result<()> {
    if fs::exists(&path)? {
        fs::remove_dir_all(&path)?;
    }

    Ok(())
}

pub fn bulletproof_unmount(path: &str) -> Result<()> {
    sync_disks()?;
    unmount(&path, UnmountFlags::DETACH | UnmountFlags::FORCE)?;

    Ok(())
}

pub fn is_mountpoint(path: &str) -> Result<bool> {
    // Could be replaced by proper Rust logic further on
    if let Err(_e) = run_command("/bin/mountpoint", &[&path]) {
        debug!("Path '{}' is not a mountpoint", &path);
        return Ok(false);
    } else {
        debug!("Path '{}' is a mountpoint", &path);
        return Ok(true);
    }
}

pub fn mount_qinit_binaries() -> Result<()> {
    let qinit_binaries_archive_path = format!(
        "{}{}",
        &crate::BOOT_PART_MOUNTPOINT,
        &QINIT_BINARIES_ARCHIVE
    );

    if !is_mountpoint(&QINIT_BINARIES_DIR_PATH)? {
        fs::create_dir_all(&QINIT_BINARIES_DIR_PATH).with_context(|| {
            format!(
                "Failed to create qinit binaries directory at '{}'",
                &QINIT_BINARIES_DIR_PATH
            )
        })?;
        run_command(
            "/bin/mount",
            &[&qinit_binaries_archive_path, &QINIT_BINARIES_DIR_PATH],
        )
        .with_context(|| "Failed to mount qinit binaries")?;
    }

    Ok(())
}

pub fn run_core_settings() -> Result<()> {
    mount_qinit_binaries()?;
    run_command(&format!("{}/core_settings", &QINIT_BINARIES_DIR_PATH), &[])?;

    Ok(())
}
