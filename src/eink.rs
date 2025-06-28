use std::fs;
use anyhow::{Context, Result};

use crate::system;

const WAVEFORM_PART: &str = "/dev/mmcblk0p2";
const WAVEFORM_FILE: &str = "ebc.wbf";
const WAVEFORM_DIR: &str = "/lib/firmware/rockchip/";
const PYTHON_SCRIPTS_PATH: &str = "/etc/init.d/ebc/";

pub fn load_waveform() -> Result<()> {
    let waveform_path = WAVEFORM_DIR.to_owned() + WAVEFORM_FILE;
    let waveform = fs::read(&WAVEFORM_PART).with_context(|| "Failed to read eInk waveform")?;
    fs::create_dir_all(&WAVEFORM_DIR).with_context(|| "Failed to create waveform's directory")?;
    fs::write(&waveform_path, &waveform).with_context(|| "Failed to write waveform to file")?;
    
    system::set_workdir(&WAVEFORM_DIR)?;
    system::run_command("python3", &[&format!("{}{}", &PYTHON_SCRIPTS_PATH, "wbf_to_custom.py"), &waveform_path]).with_context(|| "Failed to create custom waveform")?;
    system::set_workdir("/")?;

    Ok(())
}

pub fn load_modules() -> Result<()> {
    let modules = [
        "tps65185_regulator",
        "industrialio_triggered_event",
        "industrialio",
        "panel_simple",
        "rockchip_ebc",
    ];

    for module in &modules {
        system::modprobe(&[module])?;
    }

    Ok(())
}
