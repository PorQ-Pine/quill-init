use crate::system::{start_service, restart_service, run_command, modprobe};
use anyhow::{Context, Result};
use std::process::Command;
use std::fs;
use regex::Regex;
use log::info;
use std::sync::mpsc::{Receiver, Sender, channel};
use ping_rs;

const WIFI_MODULE: &str = "brcmfmac_wcc";
const WIFI_IF: &str = "wlan0";
const IWCTL_PATH: &str = "/usr/bin/iwctl";
const IWD_SERVICE: &str = "iwd";

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
    pub error: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum CommandType {
    Enable,
    Disable,
    Connect,
    Disconnect,
    GetStatus,
}

#[derive(Debug, PartialEq)]
pub struct CommandForm {
    pub command_type: CommandType,
    pub string_arguments: Option<String>,
}

pub fn daemon(wifi_status_sender: Sender<Status>, wifi_command_receiver: Receiver<CommandForm>) -> Result<()> {
    loop {
        if let Ok(command_form) = wifi_command_receiver.recv() {
            if command_form.command_type == CommandType::GetStatus {
                wifi_status_sender.send(get_status()?)?;
            }
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

    run_command(&IWCTL_PATH, &["station", &WIFI_IF, "scan"])?;
    let raw_iwd_output = Command::new(&IWCTL_PATH).args(&["station", &WIFI_IF, "get-networks"]).output()?;
    let raw_networks_list = String::from_utf8_lossy(&raw_iwd_output.stdout);

    let ansi_escape = Regex::new(r"\x1b\[[0-9;]*m")?;

    let mut lines: Vec<_> = raw_networks_list.lines().map(str::to_string).collect();
    lines = lines[4..lines.len() - 1].to_vec();

    for (i, line) in lines.iter().enumerate() {
        let mut clean_line = line.to_string();
        clean_line = ansi_escape.replace_all(line, "").trim_start().to_string();

        // Maximum SSID length for a Wi-Fi network is 32 characters, so we should be safe here
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

fn disable() -> Result<()> {
    modprobe(&["-r", &WIFI_MODULE])?;

    Ok(())
}

fn enable() -> Result<()> {
    modprobe(&[&WIFI_MODULE])?;
    run_command("/sbin/ifconfig", &[WIFI_IF, "up"])?;
    restart_service(&IWD_SERVICE)?;

    Ok(())
}

fn get_status() -> Result<Status> {
    let status;
    if fs::exists("/sys/module/brcmfmac_wcc")? {
        status = Status { status_type: StatusType::NotConnected, error: None };
    } else {
        status = Status { status_type: StatusType::Error, error: Some("Unknown".to_string()) };
    }

    Ok(status)
}
