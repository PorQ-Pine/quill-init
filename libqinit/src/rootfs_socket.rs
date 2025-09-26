use anyhow::{Context, Result};
use libquillcom::socket;
use log::info;

pub const ROOTFS_SOCKET_PATH: &str = "/overlay/run/qinit_rootfs.sock";

pub fn initialize() -> Result<()> {
    socket::bind(&ROOTFS_SOCKET_PATH)?;

    Ok(())
}
