use crate::system::{start_service, restart_service, run_command};
use anyhow::{Context, Result};
use std::process::Command;
use std::fs;
use regex::Regex;
use log::info;

const WIFI_DEV: &str = "wlan0";
const IWCTL_PATH: &str = "/usr/bin/iwctl";

#[derive(Debug)]
pub struct Network {
    name: String,
    open: bool,
    // Maybe something to implement in the future?
    // strength: i32,
}

fn start_iwd() -> Result<()> {
    if !fs::exists(&format!("{}/started/iwd", &crate::OPENRC_WORKDIR))? {
        start_service("iwd")?;
    }

    Ok(())
}

pub fn get_networks() -> Result<Vec<Network>> {
    start_iwd()?;

    let mut networks_list = Vec::new();

    run_command(&IWCTL_PATH, &["station", &WIFI_DEV, "scan"])?;
    let raw_iwd_output = Command::new(&IWCTL_PATH).args(&["station", &WIFI_DEV, "get-networks"]).output()?;
    let raw_networks_list = String::from_utf8_lossy(&raw_iwd_output.stdout);

    let ansi_escape = Regex::new(r"\x1b\[[0-9;]*m")?;

    let mut lines: Vec<_> = raw_networks_list.lines().map(str::to_string).collect();
    lines = lines[4..lines.len() - 1].to_vec();

    for (i, line) in lines.iter().enumerate() {
        let mut clean_line = line.to_string();
        clean_line = ansi_escape.replace_all(line, "").trim_start().to_string();

        let network_name_str = &clean_line[..32].trim();
        let security_str = &clean_line[34..54].trim();

        info!("{},{}", &network_name_str, &security_str);

        let mut open = false;
        if security_str.contains("open") {
            open = true;
        }

        let network = Network { name: network_name_str.to_string(), open: open };
        networks_list.push(network);
    }

    Ok(networks_list)
}
