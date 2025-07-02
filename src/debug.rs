use std::fs;
use network_interface::NetworkInterface;
use network_interface::NetworkInterfaceConfig;
use log::{info, warn, error};
use anyhow::{Context, Result};
use openssl::pkey::Public;
use openssl::pkey::PKey;

use crate::signing;
use crate::system;

const IP: &str = "192.168.2.2";
const IP_POOL_END: &str = "192.168.2.254";
const UDHCPD_CONF_PATH: &str = "/etc/udhcpd.conf";
const DROPBEAR_RSA_KEY_FILE: &str = "rsa_hkey";
const DEBUG_SETUP_SCRIPT: &str = "debug-setup.sh";
const COPIED_DEBUG_SCRIPT: &str = ".profile";

pub fn start_debug_framework(pubkey: &PKey<Public>) -> Result<()> {
    start_usbnet()?;
    start_sshd()?;
    prepare_script_login(&pubkey)?;

    Ok(())
}

pub fn start_usbnet() -> Result<()> {
    warn!("Setting up USB networking");
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
    fs::write(&UDHCPD_CONF_PATH, format!("start {}\nend {}\ninterface {}\n", &IP, &IP_POOL_END, &iface_name))?;
    system::run_command("udhcpd", &[&UDHCPD_CONF_PATH]).with_context(|| "Failed to start DHCP server")?;

    Ok(())
}

pub fn start_sshd() -> Result<()> {
    warn!("Starting SSH server");
    let dropbear_rsa_key_path = format!("{}{}{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR, &DROPBEAR_RSA_KEY_FILE);
    if !fs::exists(&dropbear_rsa_key_path)? {
        system::run_command("dropbearkey", &["-t", "rsa", "-f", &dropbear_rsa_key_path]).with_context(|| "Failed to generate SSH keys")?;
    }
    system::run_command("dropbear", &["-r", &dropbear_rsa_key_path, "-B"]).with_context(|| "Failed to start Dropbear SSH server")?;

    Ok(())
}

pub fn prepare_script_login(pubkey: &PKey<Public>) -> Result<()> {
    warn!("Looking for script to run upon console login");
    let script_path = format!("{}{}{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR, &DEBUG_SETUP_SCRIPT);
    let script_signature_path = format!("{}.dgst", &script_path);
    if fs::exists(&script_path)? && signing::check_signature(&pubkey, &script_path, &script_signature_path)? {
        warn!("Found valid script to run upon console login: copying it");
        let copied_debug_script_path = format!("{}{}", &crate::HOME_DIR, &COPIED_DEBUG_SCRIPT);
        fs::copy(&script_path, &copied_debug_script_path).with_context(|| "Failed to copy debug setup script to home directory")?;
    } else {
        warn!("Could not find valid script to run upon console login");
    }

    Ok(())
}
