use anyhow::{Context, Result};
use base64::prelude::*;
use log::{debug, info, warn};
use openssl::pkey::PKey;
use openssl::pkey::Public;
use regex::Regex;
use rmesg;
use sha256;
use std::env;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::{fs, process::Command, thread, time::Duration};
use sys_mount::{Mount, UnmountFlags, unmount};

use crate::signing::check_signature;

pub const MODULES_DIR_PATH: &str = "/lib/modules";
pub const FIRMWARE_DIR_PATH: &str = "/lib/firmware";
pub const FIRMWARE_ARCHIVE: &str = "firmware.squashfs";
pub const WAVEFORM_DIR_PATH: &str = "/lib/firmware/rockchip/";

#[derive(PartialEq)]
pub enum BootCommand {
    PowerOff,
    Reboot,
    NormalBoot,
    BootFinished,
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

pub fn mount_data_partition() -> Result<()> {
    info!("Mounting data partition");
    fs::create_dir_all(&crate::DATA_PART_MOUNTPOINT)
        .with_context(|| "Failed to create data partition mountpoint's directory")?;
    wait_for_path(&crate::DATA_PART)?;
    Mount::builder()
        .fstype("ext4")
        .data("rw")
        .mount(&crate::DATA_PART, &crate::DATA_PART_MOUNTPOINT)
        .with_context(|| "Failed to mount data partition")?;

    let boot_dir_path = format!("{}/{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR);
    fs::create_dir_all(&boot_dir_path)?;
    // Create convenient symlink
    symlink(
        &format!("{}/{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR),
        &crate::BOOT_DIR_SYMLINK_PATH,
    )
    .with_context(|| "Failed to create boot directory symlink")?;

    Ok(())
}

pub fn mount_firmware(pubkey: &PKey<Public>) -> Result<()> {
    info!("Mounting system firmware SquashFS archive");
    let firmware_archive_path = format!(
        "{}/{}/{}",
        &crate::DATA_PART_MOUNTPOINT,
        &crate::BOOT_DIR,
        &FIRMWARE_ARCHIVE
    );
    if fs::exists(&firmware_archive_path)? && check_signature(&pubkey, &firmware_archive_path)? {
        Mount::builder()
            .fstype("squashfs")
            .mount(&firmware_archive_path, &FIRMWARE_DIR_PATH)
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

pub fn unmount_data_partition() -> Result<()> {
    info!("Unmounting data partition");
    sync_disks()?;
    unmount(&crate::DATA_PART_MOUNTPOINT, UnmountFlags::DETACH)?;

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

pub fn power_off() -> Result<()> {
    warn!("Powering off");
    cfg_if::cfg_if! {
        if #[cfg(not(feature = "gui_only"))] {
            unmount_data_partition()?;
            run_command("/sbin/poweroff", &["-f"])?;
        }
    }

    Ok(())
}

pub fn reboot() -> Result<()> {
    warn!("Rebooting");
    cfg_if::cfg_if! {
        if #[cfg(not(feature = "gui_only"))] {
            unmount_data_partition()?;
            run_command("/sbin/reboot", &["-f"])?;
        }
    }

    Ok(())
}

pub fn generate_version_string(kernel_commit: &str) -> String {
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
    let version_string = format!(
        "Kernel commit: {}\n{}\n{}",
        &kernel_commit, &signing_state, &debug_state
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
    fs::remove_dir_all(&target)?;
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
    // info!("Compressing string to xz");
    let base64_string = BASE64_STANDARD.encode(string);
    let data = Command::new("/bin/sh")
        .args(&[
            "-c",
            &format!("printf '{}' | base64 -d | xz -9 -e", &base64_string),
        ])
        .output()?
        .stdout;
    // info!("Compressed string size: {} bytes", data.iter().count());

    Ok(data)
}

pub fn enforce_fb() -> Result<()> {
    // Prevent Slint from defaulting to DRM backend
    let empty_directory_path = "/.empty";
    let dri_directory_path = "/dev/dri/";
    fs::create_dir_all(&empty_directory_path)?;
    if fs::exists(&format!("{}/card0", &dri_directory_path))? {
        bind_mount(&empty_directory_path, &dri_directory_path)?;
    }

    Ok(())
}
