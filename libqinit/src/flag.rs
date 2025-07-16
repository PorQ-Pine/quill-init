use std::fs;
use anyhow::Result;
use strum_macros::AsRefStr;
use log::{info, warn, error};

pub const FLAGS_DIR: &str = "flags/";

#[derive(AsRefStr)]
pub enum Flag {
    FIRST_BOOT_DONE,
}

pub fn create_flags_dir() -> Result<()> {
    info!("Creating boot flags directory");
    fs::create_dir_all(format!("{}{}{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR, &FLAGS_DIR))?;

    Ok(())
}

pub fn read_bool(flag: Flag) -> Result<bool> {
    let flag_path = format!("{}/{}/{}/{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR, &FLAGS_DIR, &flag.as_ref());
    info!("Attempting to read boolean flag value at path '{}'", &flag_path);
    let mut value = false;
    if fs::exists(&flag_path)? {
        if fs::read_to_string(&flag_path)?.trim() == "true" {
            value = true;
        } else {
            value = false;
        }
    }
    info!("Returning '{}'", value.to_string());
    Ok(value)
}

pub fn write_bool(flag: Flag, value: bool) -> Result<()> {
    let flag_path = format!("{}/{}/{}/{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR, &FLAGS_DIR, &flag.as_ref());
    info!("Attempting to write boolean flag at path '{}' with value '{}'", &flag_path, value.to_string());
    Ok(fs::write(&flag_path, format!("{}\n", value.to_string()))?)
}
