use std::os::unix::net::UnixListener;

use anyhow::{Context, Result};
use libquillcom::socket::{self, Command};
use postcard::{from_bytes, to_allocvec};
use log::info;
use core::ops::Deref;

pub const ROOTFS_SOCKET_PATH: &str = "/overlay/run/qinit_rootfs.sock";

pub fn initialize() -> Result<()> {
    let unix_listener = socket::bind(&ROOTFS_SOCKET_PATH)?;
    listen_for_commands(unix_listener)?;

    Ok(())
}

pub fn listen_for_commands(unix_listener: UnixListener) -> Result<()> {
    info!("Listening for commands");
    loop {
        match from_bytes::<Command>(socket::read(unix_listener.try_clone()?)?.deref())? {
            Command::GetLoginCredentials => {
                info!("Sending login credentials to root filesystem");
            }
            Command::StopListening => {
                break;
            }
        }
    }

    info!("Stopped listening for commands");
    Ok(())
}
