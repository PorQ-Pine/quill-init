use std::fs;
use std::fs::File;
use anyhow::{Context, Result};
use log::{info, warn, error};
use libqinit::system::{run_command, modprobe, set_workdir, start_service};

const WAVEFORM_PART: &str = "/dev/mmcblk0p2";
const WAVEFORM_FILE: &str = "ebc.wbf";
const CUSTOMWF_FILE: &str = "custom_wf.bin";
const WAVEFORM_DIR_PATH: &str = "/lib/firmware/rockchip/";
const FIRMWARE_DIR: &str = "firmware/";
const PYTHON_SCRIPTS_PATH: &str = "/etc/init.d/ebc/";

pub fn load_waveform() -> Result<()> {
    info!("Loading waveform from MMC");
    let waveform_path = format!("{}{}", &WAVEFORM_DIR_PATH, &WAVEFORM_FILE);
    let waveform_customwf_path = format!("{}{}", &WAVEFORM_DIR_PATH, &CUSTOMWF_FILE);
    let waveform_backup_dir_path = format!("{}{}{}", &libqinit::DATA_PART_MOUNTPOINT, &libqinit::BOOT_DIR, &FIRMWARE_DIR);
    let waveform_backup_ebcwbf_path = format!("{}{}", &waveform_backup_dir_path, &WAVEFORM_FILE);
    let waveform_backup_customwf_path = format!("{}{}", &waveform_backup_dir_path, &CUSTOMWF_FILE);

    if !fs::exists(&waveform_backup_ebcwbf_path)? || !fs::exists(&waveform_backup_customwf_path)? {
        info!("Backing waveform file up to data partition");
        backup_waveform_files(&waveform_backup_dir_path, &waveform_backup_ebcwbf_path)?;
    } else {
        info!("Found existing waveform backup files");
    }

    info!("Copying backup waveform files to live system");
    fs::create_dir_all(&WAVEFORM_DIR_PATH).with_context(|| "Failed to create waveform's directory")?;
    fs::copy(&waveform_backup_ebcwbf_path, &waveform_path)?;
    fs::copy(&waveform_backup_customwf_path, &waveform_customwf_path)?;

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

pub fn create_custom_waveform(waveform_path: &str, workdir: &str) -> Result<()> {
    // TODO: Decide what we do with this
    set_workdir(&workdir)?;
    run_command("python3", &[&format!("{}{}", &PYTHON_SCRIPTS_PATH, "wbf_to_custom.py"), &waveform_path]).with_context(|| "Failed to create custom waveform")?;
    set_workdir("/")?;

    Ok(())
}

pub fn backup_waveform_files(waveform_backup_dir_path: &str, waveform_backup_ebcwbf_path: &str) -> Result<()> {
    let waveform = fs::read(&WAVEFORM_PART).with_context(|| "Failed to read waveform")?;
    fs::create_dir_all(&waveform_backup_dir_path)?;
    fs::write(&waveform_backup_ebcwbf_path, &waveform).with_context(|| "Failed to write waveform to file")?;
    info!("Creating custom waveform: this could take a while");
    create_custom_waveform(&waveform_backup_ebcwbf_path, &waveform_backup_dir_path)?;

    Ok(())
}

pub fn setup_touchscreen() -> Result<()> {
    info!("Setting up touchscreen input");

    run_command("openrc", &[])?;
    File::create("/run/openrc/softlevel")?;
    start_service("udev")?;
    start_service("udev-trigger")?;
    start_service("udev-settle")?;

    Ok(())
}
