use crate::system::{modprobe, restart_service, run_command, stop_service, sync_time};
use anyhow::Result;
use log::{error, info};
use regex::Regex;
use std::fs;
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender};

const WIFI_MODULE: &str = "brcmfmac_wcc";
const WIFI_IF: &str = "wlan0";
const IWCTL_PATH: &str = "/usr/bin/iwctl";
const IWD_SERVICE: &str = "iwd";
const MAX_SCAN_RETRIES: i32 = 30;
const MAX_PING_RETRIES: i32 = 5;
const PING_TIMEOUT_SECS: i32 = 5;

#[derive(Debug, PartialEq)]
pub struct Network {
    pub name: String,
    pub open: bool,
    pub currently_connected: bool,
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
pub struct NetworkForm {
    pub name: String,
    pub passphrase: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct CommandForm {
    pub command_type: CommandType,
    pub arguments: Option<NetworkForm>,
}

pub fn daemon(
    wifi_status_sender: Sender<Status>,
    wifi_command_receiver: Receiver<CommandForm>,
) -> Result<()> {
    loop {
        if let Ok(command_form) = wifi_command_receiver.recv() {
            info!(
                "Wi-Fi daemon: received new command: {:?}",
                &command_form.command_type
            );

            let mut wifi_status: Status;

            if command_form.command_type == CommandType::Enable {
                if let Err(e) = enable() {
                    error!("Failed to enable Wi-Fi: {}", &e);
                }
            } else if command_form.command_type == CommandType::Disable {
                if let Err(e) = disable() {
                    error!("Failed to disable Wi-Fi: {}", &e);
                }
            }

            if let Ok(wifi_status_) = get_status(false) {
                wifi_status = wifi_status_;
            } else {
                wifi_status = Status {
                    status_type: StatusType::Error,
                    list: None,
                    error: Some("Failed to get Wi-Fi status".to_string()),
                }
            }

            if command_form.command_type == CommandType::Connect {
                if let Some(network) = command_form.arguments {
                    if let Err(e) = connect(&network) {
                        wifi_status.status_type = StatusType::Error;
                        wifi_status.error = Some("Failed to connect to network".to_string());
                        error!("Failed to connect to network: {}", &e);
                    }
                } else {
                    wifi_status.status_type = StatusType::Error;
                    wifi_status.error = Some("Failed to get network details".to_string());
                }
            }
            if wifi_status.status_type != StatusType::Disabled
                && (command_form.command_type == CommandType::GetNetworks
                    || command_form.command_type == CommandType::GetStatus
                    || command_form.command_type == CommandType::Connect)
            {
                if let Ok(networks_list) = get_networks() {
                    if wifi_status.error.is_none() {
                        // If no errors were reported, get Wi-Fi status again to check whether or not we are connected to the Internet
                        if let Ok(wifi_status_) = get_status(true) {
                            wifi_status = wifi_status_;
                        } else {
                            wifi_status = Status {
                                status_type: StatusType::Error,
                                list: None,
                                error: Some("Failed to get Wi-Fi status".to_string()),
                            }
                        }
                    }
                    wifi_status.list = Some(networks_list);
                } else {
                    wifi_status.status_type = StatusType::Error;
                    wifi_status.error = Some("Failed to get networks list".to_string());
                }
            }

            wifi_status_sender.send(wifi_status)?;
        }
    }
}

fn get_networks() -> Result<Vec<Network>> {
    restart_service(&IWD_SERVICE)?;

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
        std::thread::sleep(std::time::Duration::from_millis(100));
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

        let mut final_network_name = network_name_str.to_string();
        let mut currently_connected = false;
        if final_network_name.starts_with(">   ") {
            currently_connected = true;
            final_network_name = final_network_name[4..].to_string();
        }

        let network = Network {
            name: final_network_name,
            open: open,
            currently_connected: currently_connected,
        };
        networks_list.push(network);
    }

    Ok(networks_list)
}

fn disable() -> Result<()> {
    info!("Disabling Wi-Fi");
    stop_service(&IWD_SERVICE)?;
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

    Ok(())
}

fn connect(network: &NetworkForm) -> Result<()> {
    info!(
        "Attempting to connect to network with the following credentials: {:?}",
        &network
    );
    if network.passphrase.is_none() {
        run_command(
            &IWCTL_PATH,
            &["station", &WIFI_IF, "connect", &network.name],
        )?;
    } else {
        if let Some(passphrase) = &network.passphrase {
            run_command(
                &IWCTL_PATH,
                &[
                    "--passphrase",
                    &passphrase,
                    "station",
                    &WIFI_IF,
                    "connect",
                    &network.name,
                ],
            )?;
        }
    }

    let _ = sync_time();

    Ok(())
}

fn get_status(do_ping: bool) -> Result<Status> {
    info!("Determining Wi-Fi status");
    let status;
    if fs::exists(&format!("/sys/module/{}", &WIFI_MODULE))? {
        if do_ping {
            // Give it some time for DHCP lease acquisition
            std::thread::sleep(std::time::Duration::from_secs(2));
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
    let mut retries = 0;
    loop {
        if retries < MAX_PING_RETRIES {
            if let Ok(()) = run_command(
                "/bin/ping",
                &[
                    "-w",
                    &format!("{}", &PING_TIMEOUT_SECS),
                    "-c",
                    "1",
                    "1.1.1.1",
                ],
            ) {
                return Ok(true);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            retries += 1;
        } else {
            return Ok(false);
        }
    }
}
