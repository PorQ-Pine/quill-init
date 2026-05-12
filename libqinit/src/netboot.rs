use anyhow::{Context, Result};
use log::info;
use std::process::Command;

use crate::system::{modprobe, run_command};

pub const NETBOOT_DEVICE_NODE: &str = "/dev/nbd0";

#[derive(PartialEq, Clone)]
pub enum NetBootStatus {
    None,
    Pending,
    Available,
}

pub fn find_host_ip_addr() -> Result<String> {
    let host_ip_addr;
    loop {
        let output = Command::new("/usr/bin/dumpleases")
            .output()
            .with_context(|| "Failed to get dumpleases' output")?;
        if output.status.success() {
            match String::from_utf8_lossy(&output.stdout).lines().last() {
                Some(line) => {
                    let ip_addr_vec: Vec<&str> = line.split_whitespace().collect();
                    match ip_addr_vec.get(1).map(|ip| ip.to_string()) {
                        Some(ip_addr) => {
                            host_ip_addr = ip_addr;
                            break;
                        }
                        None => continue,
                    }
                }
                None => continue,
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(250));
    }

    info!("Found NetBoot host IP address: {}", &host_ip_addr);
    Ok(host_ip_addr)
}

pub fn setup() -> Result<()> {
    modprobe(&["nbd"])?;

    let host_ip_addr = find_host_ip_addr()?;
    run_command(
        "/usr/sbin/nbd-client",
        &[&host_ip_addr, "10809", &NETBOOT_DEVICE_NODE],
    )?;

    Ok(())
}
