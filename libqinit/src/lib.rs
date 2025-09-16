use cfg_if;

cfg_if::cfg_if! {
    if #[cfg(not(feature = "init_wrapper"))] {
        pub mod recovery;
        pub mod rootfs;
        pub mod systemd;
        pub mod wifi;
        pub mod storage_encryption;
        pub mod brightness;
        pub mod battery;
        pub mod networking;
    }
}
pub mod boot_config;
pub mod eink;
pub mod signing;
pub mod system;
pub mod rootfs_socket;

pub const BOOT_PART: &str = "/dev/mmcblk0p7";
pub const MAIN_PART: &str = "/dev/mmcblk0p9";
pub const BOOT_PART_MOUNTPOINT: &str = "/boot/";
pub const MAIN_PART_MOUNTPOINT: &str = "/main/";
pub const BOOT_DIR: &str = "boot/";
pub const SYSTEM_DIR: &str = "system/";
pub const ROOTFS_DIR: &str = "rootfs/";
pub const SYSTEM_HOME_DIR: &str = "home/";
pub const DEFAULT_MOUNTPOINT: &str = "/mnt/";
pub const GENERIC_DIGEST_EXT: &str = ".dgst";
pub const HOME_DIR: &str = "/root/";
pub const ROOTFS_FILE: &str = "rootfs.squashfs";
pub const OVERLAY_WORKDIR: &str = "/.overlay/";
pub const OVERLAY_MOUNTPOINT: &str = "/overlay/";
pub const READY_PROGRESS_VALUE: f32 = 1.0;
pub const OPENRC_WORKDIR: &str = "/run/openrc";
