use crate::system::{modprobe, run_command, start_service};
use anyhow::{Context, Result};
use log::{info, warn};
use std::fs;
use std::fs::File;
use std::process::Command;

const WAVEFORM_PART: &str = "/dev/mmcblk0p2";
const WAVEFORM_FILE: &str = "ebc.wbf";
const CUSTOMWF_FILE: &str = "custom_wf.bin";
const FIRMWARE_DIR: &str = "waveform/";

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

pub fn create_custom_waveform(_waveform_path: &str, _workdir: &str) -> Result<()> {
    // TODO: Decide what we do with this

    Ok(())
}

pub fn backup_waveform_files(
    waveform_backup_dir_path: &str,
    waveform_backup_ebcwbf_path: &str,
) -> Result<()> {
    let mut waveform = fs::read(&WAVEFORM_PART).with_context(|| "Failed to read waveform")?;
    if waveform.is_empty() {
        warn!("Waveform data is empty, trying again with dd");
        waveform = Command::new("/bin/dd").args(&[&format!("if={}", &WAVEFORM_PART)]).output().with_context(|| "Failed to collect dd output")?.stdout;
    }
    if waveform.is_empty() {
        return Err(anyhow::anyhow!("Failed to read waveform using dd: waveform data is still empty"));
    }

    fs::create_dir_all(&waveform_backup_dir_path)?;
    fs::write(&waveform_backup_ebcwbf_path, &waveform)
        .with_context(|| "Failed to write waveform to file")?;
    info!("Creating custom waveform: this could take a while");
    create_custom_waveform(&waveform_backup_ebcwbf_path, &waveform_backup_dir_path)?;

    Ok(())
}

pub fn setup_touchscreen() -> Result<()> {
    info!("Setting up touchscreen input");

    run_command("/sbin/openrc", &[])?;
    File::create("/run/openrc/softlevel")?;
    start_service("udev")?;
    start_service("udev-trigger")?;
    start_service("udev-settle")?;

    Ok(())
}
