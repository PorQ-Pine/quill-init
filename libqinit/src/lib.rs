pub mod signing;
pub mod system;
pub mod socket;
pub mod flag;
pub mod rootfs;
pub mod systemd;

pub const CONSOLE_TTY: &str = "/dev/ttyS2";
pub const CONSOLE_BAUDRATE: &str = "1500000"
pub const DATA_PART: &str = "/dev/mmcblk0p6";
pub const DATA_PART_MOUNTPOINT: &str = "/data/";
pub const BOOT_DIR: &str = "boot/";
pub const DEFAULT_MOUNTPOINT: &str = "/mnt/";
pub const GENERIC_DIGEST_EXT: &str = ".dgst";
pub const HOME_DIR: &str = "/root/";
pub const ROOTFS_FILE: &str = "rootfs.squashfs";
pub const OVERLAY_WORKDIR: &str = "/.overlay/";
pub const OVERLAY_MOUNTPOINT: &str = "/overlay/";
