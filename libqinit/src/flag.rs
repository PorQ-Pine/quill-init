use std::fs;
use anyhow::Result;
use strum_macros::AsRefStr;
use log::{info, warn, error};

pub const FLAGS_DIR: &str = "flags/";

#[derive(AsRefStr)]
pub enum Flag {
    FIRST_BOOT_DONE,
    SYSTEMD_TARGETS_TOTAL,
}

pub fn create_flags_dir() -> Result<()> {
    info!("Creating boot flags directory");
    fs::create_dir_all(format!("{}{}{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR, &FLAGS_DIR))?;

    Ok(())
}

pub fn get_flag_path(flag: Flag) -> String {
    return format!("{}/{}/{}/{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR, &FLAGS_DIR, &flag.as_ref());
}

pub fn read_bool(flag: Flag) -> Result<bool> {
    let flag_path = get_flag_path(flag);
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
    let flag_path = get_flag_path(flag);
    info!("Attempting to write boolean flag at path '{}' with value '{}'", &flag_path, value.to_string());
    
    Ok(fs::write(&flag_path, format!("{}\n", value.to_string()))?)
}

pub fn read_string(flag: Flag) -> Result<String> {
    let flag_path = get_flag_path(flag);
    info!("Attempting to read string from flag at path '{}'", &flag_path);
    if fs::exists(&flag_path)? {
        return Ok(fs::read_to_string(&flag_path)?.trim().to_string())
    } else {
        return Err(anyhow::anyhow!("Failed to read flag at path '{}': flag does not exist", &flag_path))
    }
}

pub fn write_string(flag: Flag, contents: &str) -> Result<()> {
    let flag_path = get_flag_path(flag);
    info!("Attempting to write '{}' in flag at path '{}'", &contents, &flag_path);

    Ok(fs::write(&flag_path, format!("{}\n", &contents))?)
}

pub fn is_set(flag: Flag) -> Result<bool> {
    let flag_path = get_flag_path(flag);
    info!("Attempting to determine whether or not flag at path '{}' is set", &flag_path);
    if fs::exists(&flag_path)? && !fs::read_to_string(&flag_path)?.is_empty() {
        info!("Flag is set");
        return Ok(true)
    } else {
        info!("Flag is not set");
        return Ok(false)
    }
}
