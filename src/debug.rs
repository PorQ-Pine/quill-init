use std::fs;
use network_interface::NetworkInterface;
use network_interface::NetworkInterfaceConfig;
use log::{info, warn, error};
use std::io;
use anyhow::{Context, Result};

use crate::system;

const IP: &str = "192.168.2.3";
const IP_POOL_END: &str = "192.168.2.254";
const UDHCPD_CONF_PATH: &str = "/etc/udhcpd.conf";
const DROPBEAR_RSA_KEY_FILE: &str = "rsa_hkey";

pub fn start_debug_framework() -> Result<()> {
    env_logger::init();

    warn!("Setting up USB networking and Telnet server");
    // liblmod is not able to load g_ether properly, it seems
    system::modprobe(&["phy-rockchip-inno-usb2"])?;
    system::modprobe(&["g_ether"])?;

    let network_interfaces = NetworkInterface::show().with_context(|| "Failed to retrieve network interfaces")?;

    // Normally, any sane PineNote will only have a single USB ethernet interface once the g_ether module is loaded
    let iface_name = network_interfaces
        .iter()
        .find(|iface| iface.name.starts_with("usb"))
        .map(|iface| iface.name.clone())
        .with_context(|| "No USB ethernet interface found")?;

    // USB networking
    system::run_command("ifconfig", &[&iface_name, "up"]).with_context(|| "Failed to activate {iface_name}")?;
    system::run_command("ifconfig", &[&iface_name, &IP]).with_context(|| "Failed to set IP for {iface_name}")?;
    fs::write(UDHCPD_CONF_PATH, format!("start {IP}\nend {IP_POOL_END}\ninterface {iface_name}\n"))?;
    system::run_command("udhcpd", &[&UDHCPD_CONF_PATH]).with_context(|| "Failed to start DHCP server")?;

    let dropbear_rsa_key_path = crate::DATA_PART_MOUNTPOINT.to_owned() + crate::BOOT_DIR + DROPBEAR_RSA_KEY_FILE;
    if !fs::exists(&dropbear_rsa_key_path)? {
        system::run_command("dropbearkey", &["-t", "rsa", "-f", &dropbear_rsa_key_path]).with_context(|| "Failed to generate SSH keys")?;
    }
    system::run_command("dropbear", &["-r", &dropbear_rsa_key_path, "-B"]).with_context(|| "Failed to start Dropbear SSH server")?;

    Ok(())
}
