use std::fs;
use network_interface::NetworkInterface;
use network_interface::NetworkInterfaceConfig;

use crate::logging;
use crate::functions;

const IP: &str = "192.168.2.3";
const IP_POOL_END: &str = "192.168.2.254";
const UDHCPD_CONF_PATH: &str = "/etc/udhcpd.conf";

pub fn start_debug_framework() {
    logging::info("Setting up USB networking and Telnet server", &logging::MessageType::Warning);
    let phy_mod = "phy-rockchip-inno-usb2";
    let ether_mod = "g_ether";

    // liblmod is not able to load g_ether properly, it seems
    if functions::run_command("modprobe", &[phy_mod], &format!("Failed to load {phy_mod}")).unwrap_or(-1) != 0 {
        return;
    }
    if functions::run_command("modprobe", &[ether_mod], &format!("Failed to load {ether_mod}")).unwrap_or(-1) != 0 {
        return;
    }

    let network_interfaces = match NetworkInterface::show() {
        Ok(iface) => iface,
        Err(_e) => {
            logging::info(&format!("Failed to retrieve network interfaces"), &logging::MessageType::Error);
            return;
        }
    };

    // Normally, any sane PineNote will only have a single USB ethernet interface once the g_ether module is loaded
    let usb_iface = network_interfaces
        .iter()
        .find(|iface| iface.name.starts_with("usb"))
        .map(|iface| iface.name.clone());
    
    let iface_name = match usb_iface {
        Some(name) => name,
        None => {
            logging::info("No USB ethernet interface found", &logging::MessageType::Error);
            return;
        }
    };

    if functions::run_command("ifconfig", &[&iface_name, "up"], &format!("Failed to activate {iface_name}")).unwrap_or(-1) != 0 {
        return;
    }

    if functions::run_command("ifconfig", &[&iface_name, &IP], &format!("Failed to set IP for {iface_name}")).unwrap_or(-1) != 0 {
        return;
    }

    if let Err(_) = fs::write(UDHCPD_CONF_PATH, format!("start {IP}\nend {IP_POOL_END}\ninterface {iface_name}\n")) {
        logging::info(&format!("Failed to write udhcpd configuration file to {UDHCPD_CONF_PATH}"), &logging::MessageType::Error);
        return;
    }

    if functions::run_command("udhcpd", &[&UDHCPD_CONF_PATH], &format!("Failed to start DHCP server")).unwrap_or(-1) != 0 {
        return;
    }
}