use std::{fs, process::Command, thread, time::Duration};
use log::{info, warn, error};
use sys_mount::{Mount, unmount, UnmountFlags};
use anyhow::{Context, Result, Error};
use openssl::pkey::Public;
use openssl::pkey::PKey;
use std::env;
use std::path::Path;

use crate::signing;

const LIBARCHIVE_FILE: &str = "pkgs.sqsh";

pub fn run_command(command: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(command)
        .args(args)
        .status()
        .with_context(|| format!("Failed to execute command: {command}"))?;

    if status.success() {
        Ok(())
    } else {
        return Err(anyhow::anyhow!("Command `{command}` exited with status: {status}"))
    }
}

pub fn modprobe(args: &[&str]) -> Result<()> {
    run_command("modprobe", args).with_context(|| format!("Failed to load module; modprobe arguments: {:?}", args))?;

    Ok(())
}

pub fn wait_for_file(file: &str) {
    while !fs::metadata(file).is_ok() {
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn mount_data_partition() -> Result<()> {
    fs::create_dir_all(crate::DATA_PART_MOUNTPOINT)?;
    wait_for_file(crate::DATA_PART);
    Mount::builder().fstype("ext4").data("rw").mount(crate::DATA_PART, crate::DATA_PART_MOUNTPOINT)?;

    Ok(())
}

pub fn install_external_libraries(pubkey: &PKey<Public>) -> Result<()> {
    let libarchive_path = crate::DATA_PART_MOUNTPOINT.to_owned() + crate::BOOT_DIR + LIBARCHIVE_FILE;
    let libarchive_digest = format!("{}{}", libarchive_path, crate::GENERIC_DIGEST_EXT);

    if fs::exists(&libarchive_path)? {
        if signing::check_signature(pubkey, &libarchive_path, &libarchive_digest)? {
            Mount::builder().fstype("squashfs").data("").mount(libarchive_path, crate::DEFAULT_MOUNTPOINT)?;
            run_command("sh", &["-c", &format!("apk add --repositories-file=/dev/null {}*", crate::DEFAULT_MOUNTPOINT)])?;
            unmount(crate::DEFAULT_MOUNTPOINT, UnmountFlags::empty())?;
        }
    } else {
        warn!("No external libraries found");
    }

    Ok(())
}

pub fn set_workdir(path: &str) -> Result<()> {
    let root = Path::new(path);
    env::set_current_dir(&root)?;

    Ok(())
}
