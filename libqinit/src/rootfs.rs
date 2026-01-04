use anyhow::{Context, Result};
use log::{debug, info};
use openssl::pkey::PKey;
use openssl::pkey::Public;
use std::fs;
use sys_mount::Mount;

use crate::signing::check_signature;
use crate::system::{self, bind_mount, bulletproof_unmount, rm_dir_all, run_command};

pub const ROOTFS_MOUNTED_PROGRESS_VALUE: f32 = 0.1;
const RO_DIR: &str = "read/";
const RW_WRITE_DIR: &str = "write/";
const RW_MODULES_WRITE_DIR: &str = "write-modules/";
const RW_FIRMWARE_WRITE_DIR: &str = "write-firmware/";
const RW_WORK_DIR: &str = "work/";
const RW_MODULES_WORK_DIR: &str = "work-modules/";
const RW_FIRMWARE_WORK_DIR: &str = "work-firmware/";

pub fn setup(pubkey: &PKey<Public>, persistent: bool) -> Result<()> {
    info!("Mounting root filesystem SquashFS archive");
    let rootfs_file_path = format!(
        "{}/{}/{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_DIR,
        &crate::ROOTFS_FILE
    );
    if fs::exists(&rootfs_file_path)? && check_signature(&pubkey, &rootfs_file_path)? {
        fs::create_dir_all(&crate::OVERLAY_WORKDIR)
            .with_context(|| "Failed to create overlay's work directory")?;
        // Necessary to make disk space checks work in chroot (e.g. for package managers)
        Mount::builder()
            .fstype("tmpfs")
            .mount("tmpfs", &crate::OVERLAY_WORKDIR)
            .with_context(|| "Failed to mount tmpfs at overlay work directory")?;

        let ro_mountpoint = format!("{}/{}", &crate::OVERLAY_WORKDIR, &RO_DIR);
        let rw_dir_path_base;

        let rw_write_dir_path;
        let rw_modules_write_dir_path;
        let rw_firmware_write_dir_path;
        let rw_work_dir_path;
        let rw_modules_work_dir_path;
        let rw_firmware_work_dir_path;
        if persistent {
            rw_dir_path_base = format!(
                "{}/{}/{}",
                &crate::MAIN_PART_MOUNTPOINT,
                &crate::SYSTEM_DIR,
                &crate::ROOTFS_DIR,
            );
            rw_write_dir_path = format!("{}/{}", &rw_dir_path_base, &RW_WRITE_DIR);
            rw_modules_write_dir_path = format!("{}/{}", &rw_dir_path_base, &RW_MODULES_WRITE_DIR);
            rw_firmware_write_dir_path =
                format!("{}/{}", &rw_dir_path_base, &RW_FIRMWARE_WRITE_DIR);
            rw_work_dir_path = format!("{}/{}", &rw_dir_path_base, &RW_WORK_DIR);
            rw_modules_work_dir_path = format!("{}/{}", &rw_dir_path_base, &RW_MODULES_WORK_DIR);
            rw_firmware_work_dir_path = format!("{}/{}", &rw_dir_path_base, &RW_FIRMWARE_WORK_DIR);
        } else {
            rw_write_dir_path = format!("{}/{}", &crate::OVERLAY_WORKDIR, &RW_WRITE_DIR);
            rw_modules_write_dir_path =
                format!("{}/{}", &crate::OVERLAY_WORKDIR, &RW_MODULES_WRITE_DIR);
            rw_firmware_write_dir_path =
                format!("{}/{}", &crate::OVERLAY_WORKDIR, &RW_FIRMWARE_WRITE_DIR);
            rw_work_dir_path = format!("{}/{}", &crate::OVERLAY_WORKDIR, &RW_WORK_DIR);
            rw_modules_work_dir_path =
                format!("{}/{}", &crate::OVERLAY_WORKDIR, &RW_MODULES_WORK_DIR);
            rw_firmware_work_dir_path =
                format!("{}/{}", &crate::OVERLAY_MOUNTPOINT, &RW_FIRMWARE_WORK_DIR);
        }
        fs::create_dir_all(&ro_mountpoint)?;
        fs::create_dir_all(&rw_write_dir_path)?;
        fs::create_dir_all(&rw_modules_write_dir_path)?;
        fs::create_dir_all(&rw_firmware_write_dir_path)?;
        fs::create_dir_all(&rw_work_dir_path)?;
        fs::create_dir_all(&rw_modules_work_dir_path)?;
        fs::create_dir_all(&rw_firmware_work_dir_path)?;
        fs::create_dir_all(&crate::OVERLAY_MOUNTPOINT)
            .with_context(|| "Failed to create overlay mountpoint's directory")?;

        run_command("/bin/mount", &[&rootfs_file_path, &ro_mountpoint])
            .with_context(|| "Failed to mount root filesystem's SquashFS archive")?;

        info!("Setting up overlay filesystem");
        run_command(
            "/bin/mount",
            &[
                "-t",
                "overlay",
                "-o",
                &format!(
                    "lowerdir={},upperdir={},workdir={}",
                    &ro_mountpoint, &rw_write_dir_path, &rw_work_dir_path
                ),
                "none",
                &crate::OVERLAY_MOUNTPOINT,
            ],
        )
        .with_context(|| "Failed to mount overlay filesystem at overlay's mountpoint")?;
        info!("Setting up modules overlay filesystem");
        run_command(
            "/bin/mount",
            &[
                "-t",
                "overlay",
                "-o",
                &format!(
                    "lowerdir={},upperdir={},workdir={}",
                    &system::MODULES_DIR_PATH,
                    &rw_modules_write_dir_path,
                    &rw_modules_work_dir_path
                ),
                "none",
                &format!(
                    "{}/{}",
                    &crate::OVERLAY_MOUNTPOINT,
                    &system::MODULES_DIR_PATH
                ),
            ],
        )
        .with_context(|| "Failed to mount overlay filesystem at modules overlay's mountpoint")?;
        info!("Setting up firmware overlay filesystem");
        run_command(
            "/bin/mount",
            &[
                "-t",
                "overlay",
                "-o",
                &format!(
                    "lowerdir={},upperdir={},workdir={}",
                    &system::FIRMWARE_DIR_PATH,
                    &rw_firmware_write_dir_path,
                    &rw_firmware_work_dir_path
                ),
                "none",
                &format!(
                    "{}/{}",
                    &crate::OVERLAY_MOUNTPOINT,
                    &system::FIRMWARE_DIR_PATH
                ),
            ],
        )
        .with_context(|| "Failed to mount overlay filesystem at firmware overlay's mountpoint")?;
        setup_mounts()?;
    } else {
        return Err(anyhow::anyhow!(
            "Either root filesystem SquashFS archive was not found, either its signature was invalid"
        ));
    }

    Ok(())
}

pub fn tear_down() -> Result<()> {
    info!("Unmounting root filesystem overlay and cleaning up");

    bulletproof_unmount(&crate::OVERLAY_MOUNTPOINT)
        .with_context(|| "Failed to unmount root filesystem overlay directory")?;
    bulletproof_unmount(&format!("{}", &crate::OVERLAY_WORKDIR))
        .with_context(|| "Failed to unmount root filesystem overlay's work directory")?;
    rm_dir_all(&crate::OVERLAY_MOUNTPOINT)
        .with_context(|| "Failed to remove overlay mountpoint's directory")?;
    rm_dir_all(&crate::OVERLAY_WORKDIR)
        .with_context(|| "Failed to remove overlay's work directory")?;

    Ok(())
}

pub fn setup_mounts() -> Result<()> {
    info!("Mounting filesystems in overlay");

    Mount::builder()
        .fstype("proc")
        .mount("proc", &format!("{}/proc", &crate::OVERLAY_MOUNTPOINT))
        .with_context(|| "Failed to mount proc filesystem at overlay's mountpoint")?;
    Mount::builder()
        .fstype("sysfs")
        .mount("sysfs", &format!("{}/sys", &crate::OVERLAY_MOUNTPOINT))
        .with_context(|| "Failed to mount sysfs at overlay's mountpoint")?;
    Mount::builder()
        .fstype("tmpfs")
        .mount("tmpfs", &format!("{}/tmp", &crate::OVERLAY_MOUNTPOINT))
        .with_context(|| "Failed to mount tmpfs at overlay's mountpoint ('/tmp')")?;
    Mount::builder()
        .fstype("tmpfs")
        .mount("tmpfs", &format!("{}/run", &crate::OVERLAY_MOUNTPOINT))
        .with_context(|| "Failed to mount tmpfs at overlay's mountpoint ('/run')")?;
    Mount::builder()
        .fstype("devtmpfs")
        .mount("devtmpfs", &format!("{}/dev", &crate::OVERLAY_MOUNTPOINT))
        .with_context(|| "Failed to mount devtmpfs at overlay's mountpoint")?;
    bind_mount(
        &format!("{}", &crate::BOOT_PART_MOUNTPOINT),
        &format!("{}/{}", &crate::OVERLAY_MOUNTPOINT, &crate::BOOT_DIR),
    )
    .with_context(|| "Failed to bind-mount boot partition to overlay")?;
    bind_mount(
        &format!(
            "{}/{}",
            &crate::MAIN_PART_MOUNTPOINT,
            &crate::SYSTEM_HOME_DIR
        ),
        &format!("{}/{}", &crate::OVERLAY_MOUNTPOINT, &crate::SYSTEM_HOME_DIR),
    )
    .with_context(|| "Failed to bind-mount system home directory to overlay")?;

    Ok(())
}

pub fn run_chroot_command(command: &[&str]) -> Result<()> {
    debug!("Running command in chroot: {:?}", &command);

    let mut args: Vec<&str> = Vec::with_capacity(1 + command.len());
    args.push(&crate::OVERLAY_MOUNTPOINT);
    args.extend_from_slice(&command);

    run_command("/usr/sbin/chroot", &args)?;

    Ok(())
}

pub fn set_timezone(timezone: &str) -> Result<()> {
    info!("Setting overlay filesystem's timezone to '{}'", &timezone);
    Ok(run_chroot_command(&[
        "/usr/sbin/timedatectl",
        "set-timezone",
        &timezone,
    ])?)
}
