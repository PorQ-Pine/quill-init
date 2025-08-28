use anyhow::{Context, Result};
use log::info;
use openssl::pkey::PKey;
use openssl::pkey::Public;
use std::fs;
use sys_mount::Mount;

use crate::boot_config::BootConfig;
use crate::signing::check_signature;
use crate::system::{self, bind_mount, run_command};

pub const ROOTFS_MOUNTED_PROGRESS_VALUE: f32 = 0.1;
const RW_WRITE_DIR: &str = "write/";
const RW_WORK_DIR: &str = "work/";

pub fn setup(pubkey: &PKey<Public>, boot_config: &mut BootConfig) -> Result<()> {
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

        let ro_mountpoint = format!("{}/{}", &crate::OVERLAY_WORKDIR, "read");
        let rw_write_dir_path;
        let rw_work_dir_path;
        if boot_config.rootfs.persistent_storage {
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
        setup_misc(boot_config)?;
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

pub fn change_user_password(
    pubkey: &PKey<Public>,
    user: &str,
    old_password: &str,
    new_password: &str,
) -> Result<()> {
    let rw_write_dir_path = format!(
        "{}/{}/{}/{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_DIR,
        &crate::ROOTFS_DIR,
        &RW_WRITE_DIR
    );
    fs::create_dir_all(&rw_write_dir_path)?;

    let password_temp_chroot = "/tmp/password";
    let musl_lib_path = "/lib/ld-musl-aarch64.so.1";
    let busybox_path = "/bin/busybox";
    let passwd_path_base = "/etc/passwd";
    let passwd_path = format!("{}/{}", &rw_write_dir_path, &passwd_path_base);

    if !fs::exists(&passwd_path)? {
        let rootfs_file_path = format!(
            "{}/{}/{}",
            &crate::MAIN_PART_MOUNTPOINT,
            &crate::SYSTEM_DIR,
            &crate::ROOTFS_FILE
        );
        if fs::exists(&rootfs_file_path)? && check_signature(&pubkey, &rootfs_file_path)? {
            run_command(
                "/bin/mount",
                &[&rootfs_file_path, &crate::DEFAULT_MOUNTPOINT],
            )?;
            let passwd_ro_path = format!("{}/{}", &crate::DEFAULT_MOUNTPOINT, &passwd_path_base);
            fs::copy(&passwd_ro_path, &passwd_path)?;
            run_command("/bin/umount", &[&crate::DEFAULT_MOUNTPOINT])?;
        }
    }

    let chroot_musl_lib_path = format!("{}/{}", &password_temp_chroot, &musl_lib_path);
    let chroot_busybox_path = format!("{}/{}", &password_temp_chroot, &busybox_path);
    let chroot_passwd_path = format!("{}/{}", &password_temp_chroot, &passwd_path_base);

    fs::create_dir_all(format!("{}/lib", &password_temp_chroot))?;
    fs::create_dir_all(format!("{}/bin", &password_temp_chroot))?;
    fs::create_dir_all(format!("{}/etc", &password_temp_chroot))?;
    bind_mount(&musl_lib_path, &chroot_musl_lib_path)?;
    bind_mount(&busybox_path, &chroot_busybox_path)?;
    bind_mount(&passwd_path, &chroot_passwd_path)?;

    run_command("/usr/sbin/chroot", &[&password_temp_chroot, "/bin/busybox"])?;

    Ok(())
}
