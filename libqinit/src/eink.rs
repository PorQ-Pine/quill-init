use crate::boot_config::BootConfig;
use crate::system::{modprobe, run_command, start_service};
use anyhow::{Context, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::{fs::{self, File}, thread};
use std::process::Command;

const WAVEFORM_PART: &str = "/dev/mmcblk0p2";
const WAVEFORM_FILE: &str = "ebc.wbf";
const CUSTOMWF_FILE: &str = "custom_wf.bin";
const FIRMWARE_DIR: &str = "waveform/";

const UDEV_RULES_PATH: &str = "/etc/udev/rules.d/";
const LIBINPUT_CW_0: &str = r#"ENV{LIBINPUT_CALIBRATION_MATRIX}="-1 0 1 0 -1 1""#;
const LIBINPUT_CW_90: &str = r#"ENV{LIBINPUT_CALIBRATION_MATRIX}="0 -1 1 1 0 0""#;
const LIBINPUT_CW_180: &str = r#"ENV{LIBINPUT_CALIBRATION_MATRIX}="1 0 0 0 1 0""#;
const LIBINPUT_CW_270: &str = r#"ENV{LIBINPUT_CALIBRATION_MATRIX}="0 1 0 -1 0 1""#;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub enum ScreenRotation {
    Cw0,
    Cw90,
    Cw180,
    #[default]
    Cw270,
}

pub fn load_waveform() -> Result<()> {
    info!("Loading waveform from MMC");
    let waveform_path = format!("{}/{}", &crate::system::WAVEFORM_DIR_PATH, &WAVEFORM_FILE);
    let waveform_customwf_path =
        format!("{}/{}", &crate::system::WAVEFORM_DIR_PATH, &CUSTOMWF_FILE);
    let waveform_backup_dir_path = format!("{}/{}", &crate::BOOT_PART_MOUNTPOINT, &FIRMWARE_DIR);
    let waveform_backup_ebcwbf_path = format!("{}/{}", &waveform_backup_dir_path, &WAVEFORM_FILE);
    let waveform_backup_customwf_path = format!("{}/{}", &waveform_backup_dir_path, &CUSTOMWF_FILE);

    if !fs::exists(&waveform_backup_ebcwbf_path)? || !fs::exists(&waveform_backup_customwf_path)? {
        info!("Backing waveform file up to data partition");
        backup_waveform_files(&waveform_backup_dir_path, &waveform_backup_ebcwbf_path)
            .with_context(|| "Failed to backup waveform files")?;
    } else {
        info!("Found existing waveform backup files");
    }

    info!("Copying backup waveform files to live system");
    fs::copy(&waveform_backup_ebcwbf_path, &waveform_path)
        .with_context(|| "Failed to copy backup waveform file to live system")?;
    fs::copy(&waveform_backup_customwf_path, &waveform_customwf_path)
        .with_context(|| "Failed to copy custom waveform file to live system")?;

    Ok(())
}

pub fn load_modules() -> Result<()> {
    info!("Loading eInk display modules and activating EPDC");
    let modules = [
        "tps65185_regulator",
        "industrialio_triggered_event",
        "industrialio",
        "panel_simple",
        "rockchip_ebc",
    ];

    for module in &modules {
        modprobe(&[module])?;
    }

    Ok(())
}

pub fn backup_waveform_files(
    waveform_backup_dir_path: &str,
    waveform_backup_ebcwbf_path: &str,
) -> Result<()> {
    let mut waveform = fs::read(&WAVEFORM_PART).with_context(|| "Failed to read waveform")?;
    if waveform.is_empty() {
        warn!("Waveform data is empty, trying again with dd");
        waveform = Command::new("/bin/dd")
            .args(&[&format!("if={}", &WAVEFORM_PART)])
            .output()
            .with_context(|| "Failed to collect dd output")?
            .stdout;
    }
    if waveform.is_empty() {
        return Err(anyhow::anyhow!(
            "Failed to read waveform using dd: waveform data is still empty"
        ));
    }

    fs::create_dir_all(&waveform_backup_dir_path)?;
    fs::write(&waveform_backup_ebcwbf_path, &waveform)
        .with_context(|| "Failed to write waveform to file")?;

    Ok(())
}

pub fn setup_touchscreen(boot_config: &mut BootConfig) -> Result<()> {
    info!("Setting up touchscreen input");

    fs::create_dir_all(&UDEV_RULES_PATH)?;
    let libinput_rules_path = format!("{}/libinput.rules", &UDEV_RULES_PATH);

    if boot_config.system.initial_screen_rotation == ScreenRotation::Cw0 {
        fs::write(&libinput_rules_path, &LIBINPUT_CW_0)?;
    } else if boot_config.system.initial_screen_rotation == ScreenRotation::Cw90 {
        fs::write(&libinput_rules_path, &LIBINPUT_CW_90)?;
    } else if boot_config.system.initial_screen_rotation == ScreenRotation::Cw180 {
        fs::write(&libinput_rules_path, &LIBINPUT_CW_180)?;
    } else {
        fs::write(&libinput_rules_path, &LIBINPUT_CW_270)?;
    }

    run_command("/sbin/openrc", &[])?;
    File::create("/run/openrc/softlevel")?;
    start_service("udev")?;
    start_service("udev-trigger")?;
    start_service("udev-settle")?;

    Ok(())
}

pub fn full_refresh() {
    debug!("Triggering full screen refresh");
    // Calling new here is, well, bad (because of possible wrong default values),
    // but we shut down in a second, so no one should care
    let ebc = pinenote_service::drivers::rockchip_ebc::RockchipEbc::new();
    ebc.global_refresh().ok();
    // TODO: Find a way to interact with EPDC so that it tells us when screen updates
    // are done to avoid doing this kind of horrible things
    thread::sleep(std::time::Duration::from_millis(1000));
}
