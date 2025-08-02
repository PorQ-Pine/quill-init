use cfg_if;

cfg_if::cfg_if! {
    if #[cfg(not(feature = "init_wrapper"))] {
        pub mod boot_config;
        pub mod recovery;
        pub mod rootfs;
        pub mod signing;
        pub mod system;
        pub mod systemd;
        pub mod wifi;
    }
}
pub mod socket;

pub const DATA_PART: &str = "/dev/mmcblk0p6";
pub const DATA_PART_MOUNTPOINT: &str = "/data/";
pub const BOOT_DIR: &str = "boot/";
pub const BOOT_DIR_SYMLINK_PATH: &str = "/boot";
pub const DEFAULT_MOUNTPOINT: &str = "/mnt/";
pub const GENERIC_DIGEST_EXT: &str = ".dgst";
pub const HOME_DIR: &str = "/root/";
pub const ROOTFS_FILE: &str = "rootfs.squashfs";
pub const OVERLAY_WORKDIR: &str = "/.overlay/";
pub const OVERLAY_MOUNTPOINT: &str = "/overlay/";
pub const READY_PROGRESS_VALUE: f32 = 1.0;
pub const OPENRC_WORKDIR: &str = "/run/openrc";
