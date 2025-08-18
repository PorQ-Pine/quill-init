use anyhow::{Context, Result};
use local_ip_address::list_afinet_netifas;
use log::info;

pub fn get_if_ip_address(interface: &str) -> Result<String> {
    let network_interfaces =
        list_afinet_netifas().with_context(|| "Failed to list network interfaces")?;

    for (name, ip) in network_interfaces.iter() {
        if name == interface {
            info!("IP address of interface {} is {}", &name, &ip);
            return Ok(ip.to_string());
        }
    }

    return Ok("Not found".to_string());
}
