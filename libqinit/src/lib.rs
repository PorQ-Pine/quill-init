pub mod signing;
pub mod system;
pub mod socket;

pub const DATA_PART: &str = "/dev/mmcblk0p6";
pub const DATA_PART_MOUNTPOINT: &str = "/data/";
pub const BOOT_DIR: &str = "boot/";
pub const DEFAULT_MOUNTPOINT: &str = "/mnt/";
pub const GENERIC_DIGEST_EXT: &str = ".dgst";
pub const HOME_DIR: &str = "/root/";
pub const FLAGS_DIR: &str = "flags/";
pub const ROOTFS_FILE: &str = "rootfs.squashfs";
pub const OVERLAY_WORKDIR: &str = "/.overlay/";
pub const OVERLAY_MOUNTPOINT: &str = "/overlay/";
