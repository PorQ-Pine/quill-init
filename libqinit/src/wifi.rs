use crate::system::{modprobe, restart_service, run_command, start_service};
use anyhow::{Context, Result};
use log::info;
use regex::Regex;
use std::fs;
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender, channel};

const WIFI_MODULE: &str = "brcmfmac_wcc";
const WIFI_IF: &str = "wlan0";
const IWCTL_PATH: &str = "/usr/bin/iwctl";
const IWD_SERVICE: &str = "iwd";
const MAX_SCAN_RETRIES: i32 = 5;

#[derive(Debug, PartialEq)]
pub struct Network {
    pub name: String,
    pub open: bool,
    // Maybe something to implement in the future?
    // strength: i32,
}

#[derive(Debug, PartialEq)]
pub enum StatusType {
    Disabled,
    NotConnected,
    Connected,
    Error,
}

#[derive(Debug, PartialEq)]
pub struct Status {
    pub status_type: StatusType,
    pub list: Option<Vec<Network>>,
    pub error: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum CommandType {
    Enable,
    Disable,
    Connect,
    Disconnect,
    GetStatus,
    GetNetworks,
}

#[derive(Debug, PartialEq)]
pub struct CommandForm {
    pub command_type: CommandType,
    pub string_arguments: Option<String>,
}

pub fn daemon(
    wifi_status_sender: Sender<Status>,
    wifi_command_receiver: Receiver<CommandForm>,
) -> Result<()> {
    loop {
        if let Ok(command_form) = wifi_command_receiver.recv() {
            if command_form.command_type == CommandType::Enable {
                enable()?;
            } else if command_form.command_type == CommandType::Disable {
                disable()?;
            }

            let mut wifi_status = get_status()?;

            if command_form.command_type == CommandType::GetNetworks {
                wifi_status.list = Some(get_networks()?);
            }

            wifi_status_sender.send(wifi_status)?;
        }
    }
}

fn start_iwd() -> Result<()> {
    if !fs::exists(&format!("{}/started/iwd", &crate::OPENRC_WORKDIR))? {
        start_service("iwd")?;
    }

    Ok(())
}

fn get_networks() -> Result<Vec<Network>> {
    start_iwd()?;

    let mut networks_list = Vec::new();

    let mut scan_retries = 0;
    loop {
        if scan_retries < MAX_SCAN_RETRIES {
            if let Ok(()) = run_command(&IWCTL_PATH, &["station", &WIFI_IF, "scan"]) {
                break;
            }
        } else {
            return Err(anyhow::anyhow!("Failed to scan for networks"));
        }
        scan_retries += 1;
    }

    let raw_iwd_output = Command::new(&IWCTL_PATH)
        .args(&["station", &WIFI_IF, "get-networks"])
        .output()?;
    let raw_networks_list = String::from_utf8_lossy(&raw_iwd_output.stdout);

    let ansi_escape = Regex::new(r"\x1b\[[0-9;]*m")?;

    let mut lines: Vec<_> = raw_networks_list.lines().map(str::to_string).collect();
    lines = lines[4..lines.len() - 1].to_vec();

    for (_i, line) in lines.iter().enumerate() {
        let clean_line = ansi_escape.replace_all(line, "").trim_start().to_string();

        // Maximum SSID length for a Wi-Fi network is 32 characters, so we should be safe here
        let network_name_str = &clean_line[..32].trim();
        let security_str = &clean_line[34..54].trim();

        let mut open = false;
        if security_str.contains("open") {
            open = true;
        }

        let network = Network {
            name: network_name_str.to_string(),
            open: open,
        };
        networks_list.push(network);
    }

    Ok(networks_list)
}

fn disable() -> Result<()> {
    info!("Disabling Wi-Fi");
    modprobe(&["-r", &WIFI_MODULE])?;

    Ok(())
}

fn enable() -> Result<()> {
    info!("Enabling Wi-Fi");
    modprobe(&[&WIFI_MODULE])?;
    // Wait for Wi-Fi interface to appear before trying to enable it
    loop {
        if fs::exists(&format!("/sys/class/net/{}", &WIFI_IF))? {
            run_command("/sbin/ifconfig", &[WIFI_IF, "up"])?;
            break;
        } else {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    restart_service(&IWD_SERVICE)?;

    Ok(())
}

fn get_status() -> Result<Status> {
    let status;
    if fs::exists(&format!("/sys/module/{}", &WIFI_MODULE))? {
        if is_connected_to_internet()? {
            status = Status {
                status_type: StatusType::Connected,
                list: None,
                error: None,
            };
        } else {
            status = Status {
                status_type: StatusType::NotConnected,
                list: None,
                error: None,
            };
        }
    } else {
        status = Status {
            status_type: StatusType::Disabled,
            list: None,
            error: None,
        };
    }

    Ok(status)
}

fn is_connected_to_internet() -> Result<bool> {
    if let Err(_e) = run_command("/bin/ping", &["-c", "1", "1.1.1.1"]) {
        return Ok(false);
    } else {
        return Ok(true);
    }
}
