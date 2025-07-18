use anyhow::{Context, Result};
use fs_extra::dir;
use log::{error, info, warn};
use openssl::pkey::PKey;
use openssl::pkey::Public;
use std::fs;
use sys_mount::Mount;

use crate::flags::Flags;
use crate::signing::check_signature;
use crate::system::{self, bind_mount, run_command};

pub const ROOTFS_MOUNTED_PROGRESS_VALUE: f32 = 0.1;

pub fn setup(pubkey: &PKey<Public>, flags: &mut Flags) -> Result<()> {
    info!("Mounting root filesystem SquashFS archive");
    let rootfs_file_path = format!(
        "{}/{}/{}",
        &crate::DATA_PART_MOUNTPOINT,
        &crate::BOOT_DIR,
        &crate::ROOTFS_FILE
    );
    if fs::exists(&rootfs_file_path)? && check_signature(&pubkey, &rootfs_file_path)? {
        fs::create_dir_all(&crate::OVERLAY_WORKDIR)?;
        // Necessary to make disk space checks work in chroot (e.g. for package managers)
        Mount::builder()
            .fstype("tmpfs")
            .mount("tmpfs", &crate::OVERLAY_WORKDIR)?;

        let ro_mountpoint = format!("{}/{}", &crate::OVERLAY_WORKDIR, "read");
        let rw_writedir = format!("{}/{}", &crate::OVERLAY_WORKDIR, "write");
        let rw_workdir = format!("{}/{}", &crate::OVERLAY_WORKDIR, "work");
        fs::create_dir_all(&ro_mountpoint)?;
        fs::create_dir_all(&rw_writedir)?;
        fs::create_dir_all(&rw_workdir)?;
        fs::create_dir_all(&crate::OVERLAY_MOUNTPOINT)?;

        Mount::builder()
            .fstype("squashfs")
            .mount(&rootfs_file_path, &ro_mountpoint)?;
        info!("Setting up fuse-overlayfs overlay");
        run_command(
            "/usr/bin/fuse-overlayfs",
            &[
                "-o",
                &format!(
                    "allow_other,lowerdir={},upperdir={},workdir={}",
                    &ro_mountpoint, &rw_writedir, &rw_workdir
                ),
                &crate::OVERLAY_MOUNTPOINT,
            ],
        )?;
        setup_mounts()?;
        setup_misc(flags)?;
    } else {
        return Err(anyhow::anyhow!(
            "Either root filesystem SquashFS archive was not found, either its signature was invalid"
        ));
    }

    Ok(())
}

pub fn setup_mounts() -> Result<()> {
    info!("Mounting filesystems in fuse-overlayfs overlay");

    Mount::builder()
        .fstype("proc")
        .mount("proc", &format!("{}/proc", &crate::OVERLAY_MOUNTPOINT))?;
    Mount::builder()
        .fstype("sysfs")
        .mount("sysfs", &format!("{}/sys", &crate::OVERLAY_MOUNTPOINT))?;
    Mount::builder()
        .fstype("tmpfs")
        .mount("tmpfs", &format!("{}/tmp", &crate::OVERLAY_MOUNTPOINT))?;
    Mount::builder()
        .fstype("tmpfs")
        .mount("tmpfs", &format!("{}/run", &crate::OVERLAY_MOUNTPOINT))?;
    Mount::builder()
        .fstype("devtmpfs")
        .mount("devtmpfs", &format!("{}/dev", &crate::OVERLAY_MOUNTPOINT))?;
    bind_mount(
        &format!("{}/{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR),
        &format!("{}/{}", &crate::OVERLAY_MOUNTPOINT, &crate::BOOT_DIR),
    )?;
    bind_mount(
        &system::MODULES_DIR_PATH,
        &format!(
            "{}/{}",
            &crate::OVERLAY_MOUNTPOINT,
            &system::MODULES_DIR_PATH
        ),
    )?;
    bind_mount(
        &system::FIRMWARE_DIR_PATH,
        &format!(
            "{}/{}",
            &crate::OVERLAY_MOUNTPOINT,
            &system::FIRMWARE_DIR_PATH
        ),
    )?;

    Ok(())
}

pub fn setup_misc(flags: &mut Flags) -> Result<()> {
    let first_boot_done = flags.first_boot_done;
    if !first_boot_done {
        info!("Running first boot setup commands, if any");
        flags.first_boot_done = true;
    }

    Ok(())
}

pub fn run_chroot_command(command: &[&str]) -> Result<()> {
    let mut args: Vec<&str> = Vec::with_capacity(1 + command.len());
    args.push(&crate::OVERLAY_MOUNTPOINT);
    args.extend_from_slice(&command);

    run_command("/usr/sbin/chroot", &args)?;

    Ok(())
}
