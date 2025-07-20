use anyhow::{Context, Result};
use libqinit::signing::check_signature;
use libqinit::system::{modprobe, run_command};
use log::warn;
use network_interface::NetworkInterface;
use network_interface::NetworkInterfaceConfig;
use openssl::pkey::PKey;
use openssl::pkey::Public;
use regex::Regex;
use std::fs;

const IP_ADDR: &str = "192.168.2.2";
const IP_POOL_END: &str = "192.168.2.254";
const UDHCPD_CONF_PATH: &str = "/etc/udhcpd.conf";
const DROPBEAR_RSA_KEY_FILE: &str = "rsa_hkey";
const DEBUG_SETUP_SCRIPT: &str = "debug-setup.sh";
const COPIED_DEBUG_SCRIPT: &str = ".profile";
const USER_UDHCPD_CONF_FILE: &str = "udhcpd.conf";

pub fn start_debug_framework(pubkey: &PKey<Public>) -> Result<()> {
    start_usbnet(&pubkey)?;
    start_sshd()?;
    prepare_script_login(&pubkey)?;

    Ok(())
}

pub fn start_usbnet(pubkey: &PKey<Public>) -> Result<()> {
    warn!("Setting up USB networking");
    // liblmod is not able to load g_ether properly, it seems
    modprobe(&["phy-rockchip-inno-usb2"])?;
    modprobe(&["g_ether"])?;

    let network_interfaces =
        NetworkInterface::show().with_context(|| "Failed to retrieve network interfaces")?;
    // To extract base device IP from custom udhcpd configuration (if present)
    let ip_regex = Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b")?;
    let user_udhcpd_conf_path = format!(
        "{}/{}/{}",
        &libqinit::DATA_PART_MOUNTPOINT,
        &libqinit::BOOT_DIR,
        &USER_UDHCPD_CONF_FILE
    );

    // Normally, any sane PineNote will only have a single USB ethernet interface once the g_ether module is loaded
    let iface_name = network_interfaces
        .iter()
        .find(|iface| iface.name.starts_with("usb"))
        .map(|iface| iface.name.clone())
        .with_context(|| "No USB ethernet interface found")?;

    // USB networking
    run_command("/sbin/ifconfig", &[&iface_name, "up"])
        .with_context(|| format!("Failed to activate {}", &iface_name))?;
    if fs::exists(&user_udhcpd_conf_path)? && check_signature(&pubkey, &user_udhcpd_conf_path)? {
        warn!("Found valid udhcpd user configuration file: copying it");
        fs::copy(&user_udhcpd_conf_path, &UDHCPD_CONF_PATH)?;
    } else {
        fs::write(
            &UDHCPD_CONF_PATH,
            format!(
                "start {}\nend {}\ninterface {}\n",
                &IP_ADDR, &IP_POOL_END, &iface_name
            ),
        )?;
    }
    // udhcpd configuration
    let udhcpd_config = fs::read_to_string(&UDHCPD_CONF_PATH)?;
    if let Some(custom_ip_addr_r) = ip_regex.find(&udhcpd_config) {
        let custom_ip_addr = custom_ip_addr_r.as_str();
        run_command("/sbin/ifconfig", &[&iface_name, &custom_ip_addr]).with_context(|| {
            format!(
                "Failed to set custom IP address {} for {}",
                &custom_ip_addr, &iface_name
            )
        })?;
    } else {
        run_command("/sbin/ifconfig", &[&iface_name, &IP_ADDR]).with_context(|| {
            format!("Failed to set IP address {} for {}", &IP_ADDR, &iface_name)
        })?;
    }
    run_command("/usr/sbin/udhcpd", &[&UDHCPD_CONF_PATH])
        .with_context(|| "Failed to start DHCP server")?;

    Ok(())
}

pub fn start_sshd() -> Result<()> {
    warn!("Starting SSH server");
    let dropbear_rsa_key_path = format!(
        "{}/{}/{}",
        &libqinit::DATA_PART_MOUNTPOINT,
        &libqinit::BOOT_DIR,
        &DROPBEAR_RSA_KEY_FILE
    );
    if !fs::exists(&dropbear_rsa_key_path)? {
        run_command(
            "/usr/bin/dropbearkey",
            &["-t", "rsa", "-f", &dropbear_rsa_key_path],
        )
        .with_context(|| "Failed to generate SSH keys")?;
    }
    run_command(
        "/usr/sbin/dropbear",
        &["-p", "2222", "-r", &dropbear_rsa_key_path, "-B"],
    )
    .with_context(|| "Failed to start Dropbear SSH server")?;

    Ok(())
}

pub fn prepare_script_login(pubkey: &PKey<Public>) -> Result<()> {
    warn!("Looking for script to run upon console login");
    let script_path = format!(
        "{}/{}/{}",
        &libqinit::DATA_PART_MOUNTPOINT,
        &libqinit::BOOT_DIR,
        &DEBUG_SETUP_SCRIPT
    );
    if fs::exists(&script_path)? && check_signature(&pubkey, &script_path)? {
        warn!("Found valid script to run upon console login: copying it");
        let copied_debug_script_path = format!("{}/{}", &libqinit::HOME_DIR, &COPIED_DEBUG_SCRIPT);
        fs::copy(&script_path, &copied_debug_script_path)
            .with_context(|| "Failed to copy debug setup script to home directory")?;
    } else {
        warn!("Could not find valid script to run upon console login");
    }

    Ok(())
}
