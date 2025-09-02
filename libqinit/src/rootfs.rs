use anyhow::{Context, Result};
use log::{error, info};
use openssl::pkey::PKey;
use openssl::pkey::Public;
use std::fs;
use sys_mount::Mount;

use crate::boot_config::BootConfig;
use crate::signing::check_signature;
use crate::system::bulletproof_unmount;
use crate::system::{self, bind_mount, generate_random_string, rm_dir_all, run_command};

pub const ROOTFS_MOUNTED_PROGRESS_VALUE: f32 = 0.1;
const RO_DIR: &str = "read/";
const RW_WRITE_DIR: &str = "write/";
const RW_WORK_DIR: &str = "work/";

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
        let rw_write_dir_path;
        let rw_work_dir_path;
        if persistent {
            rw_write_dir_path = format!(
                "{}/{}/{}/{}",
                &crate::MAIN_PART_MOUNTPOINT,
                &crate::SYSTEM_DIR,
                &crate::ROOTFS_DIR,
                &RW_WRITE_DIR,
            );
            rw_work_dir_path = format!(
                "{}/{}/{}/{}",
                &crate::MAIN_PART_MOUNTPOINT,
                &crate::SYSTEM_DIR,
                &crate::ROOTFS_DIR,
                &RW_WORK_DIR,
            );
        } else {
            rw_write_dir_path = format!("{}/{}", &crate::OVERLAY_WORKDIR, "write");
            rw_work_dir_path = format!("{}/{}", &crate::OVERLAY_WORKDIR, "work");
        }
        fs::create_dir_all(&ro_mountpoint)?;
        fs::create_dir_all(&rw_write_dir_path)?;
        fs::create_dir_all(&rw_work_dir_path)?;
        fs::create_dir_all(&crate::OVERLAY_MOUNTPOINT)
            .with_context(|| "Failed to create overlay mountpoint's directory")?;

        run_command("/bin/mount", &[&rootfs_file_path, &ro_mountpoint])
            .with_context(|| "Failed to mount root filesystem's SquashFS archive")?;

        bind_mount(
            &system::MODULES_DIR_PATH,
            &format!("{}/{}", &ro_mountpoint, &system::MODULES_DIR_PATH),
        )?;
        bind_mount(
            &system::FIRMWARE_DIR_PATH,
            &format!("{}/{}", &ro_mountpoint, &system::FIRMWARE_DIR_PATH),
        )?;

        info!("Setting up fuse-overlayfs overlay");
        run_command(
            "/usr/bin/fuse-overlayfs",
            &[
                "-o",
                &format!(
                    "allow_other,lowerdir={},upperdir={},workdir={}",
                    &ro_mountpoint, &rw_write_dir_path, &rw_work_dir_path
                ),
                &crate::OVERLAY_MOUNTPOINT,
            ],
        )
        .with_context(|| "Failed to mount fuse-overlayfs filesystem at overlay's mountpoint")?;
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

    bulletproof_unmount(&crate::OVERLAY_MOUNTPOINT).with_context(|| "Failed to unmount root filesystem overlay directory")?;
    bulletproof_unmount(&format!("{}", &crate::OVERLAY_WORKDIR)).with_context(|| "Failed to unmount root filesystem overlay's work directory")?;
    rm_dir_all(&crate::OVERLAY_MOUNTPOINT).with_context(|| "Failed to remove overlay mountpoint's directory")?;
    rm_dir_all(&crate::OVERLAY_WORKDIR).with_context(|| "Failed to remove overlay's work directory")?;

    Ok(())
}

pub fn setup_mounts() -> Result<()> {
    info!("Mounting filesystems in fuse-overlayfs overlay");

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
    )?;

    Ok(())
}

pub fn setup_misc(boot_config: &mut BootConfig) -> Result<()> {
    let first_boot_done = boot_config.flags.first_boot_done;
    if !first_boot_done {
        info!("Running first boot setup commands, if any");
        boot_config.flags.first_boot_done = true;
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

fn change_user_password_chroot_command(
    chroot_path: &str,
    user: &str,
    old_password: &str,
    new_password: &str,
    verify: bool,
) -> Result<()> {
    let passwd_path = "/usr/bin/passwd";
    if verify {
        run_command(
            "/usr/sbin/chroot",
            &[
                &chroot_path,
                "/bin/su",
                "-s",
                "/bin/sh",
                "-c",
                &format!("printf '{}\n{}\n{}' | {} {}", &old_password, &new_password, &new_password, &passwd_path, &user),
                &user
            ],
        ).with_context(|| "Provided login credentials were incorrect")?;
    } else {
        run_command(
            "/usr/sbin/chroot",
            &[
                &chroot_path,
                "/bin/sh",
                "-c",
                &format!("printf '{}\n{}' | {} {}", &new_password, &new_password, &passwd_path, &user)
            ],
        ).with_context(|| "Error setting password")?;
    }

    Ok(())
}

pub fn change_user_password(
    pubkey: &PKey<Public>,
    user: &str,
    old_password: &str,
    new_password: &str,
) -> Result<()> {
    info!(
        "Attempting to change system user password for user '{}'",
        &user
    );

    // Overlay should never be mounted when this function is called
    setup(&pubkey, true)?;

    let temporary_password = generate_random_string(128)?;
    info!("Temporary password is '{}'", &temporary_password);

    let mut do_error = false;
    info!("Setting temporary password for verification");
    if let Err(e) = change_user_password_chroot_command(
        &crate::OVERLAY_MOUNTPOINT,
        &user,
        &old_password,
        &temporary_password,
        true,
    ) {
        do_error = true;
        error!("{}", &e);
    } else {
        info!("Setting new requested password");
        if let Err(e) = change_user_password_chroot_command(
            &crate::OVERLAY_MOUNTPOINT,
            &user,
            &temporary_password,
            &new_password,
            false,
        ) {
            do_error = true;
            error!("{}", &e);
        }
    }

    tear_down()?;

    if do_error {
        return Err(anyhow::anyhow!(
            "Failed to set new password for user '{}'",
            &user
        ));
    }

    Ok(())
}
