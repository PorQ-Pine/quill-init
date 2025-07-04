use anyhow::{Context, Result};
use std::env;
use std::path::Path;
use std::{fs, process::Command, thread, time::Duration};
use log::{info, warn, error};
use sys_mount::Mount;
use regex::Regex;

pub fn get_cmdline_bool(property: &str) -> Result<bool> {
    info!("Trying to extract boolean value for property '{}' in kernel command line", &property);
    let cmdline = fs::read_to_string("/proc/cmdline")?;
    let re_str = format!(r"{}=(\w+)", regex::escape(&property));
    let re = Regex::new(&re_str)?;
    if let Some(captures) = re.captures(&cmdline) {
        if let Some(value_match) = captures.get(1) {
            let value = value_match.as_str();
            if value == "1" || value == "true" {
                info!("Property is true");
                return Ok(true)
            } else {
                info!("Property is false");
                return Ok(false)
            }
        } else {
            info!("Error getting capture group: returning false");
            return Ok(false)
        }
    } else {
        info!("Error capturing property: returning false");
        return Ok(false)
    }
}

pub fn set_workdir(path: &str) -> Result<()> {
    let root = Path::new(path);
    env::set_current_dir(&root)?;

    Ok(())
}

pub fn wait_for_file(file: &str) {
    while !fs::metadata(file).is_ok() {
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn run_command(command: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(&command)
        .args(args)
        .status()
        .with_context(|| format!("Failed to execute command: {}", &command))?;

    if status.success() {
        Ok(())
    } else {
        return Err(anyhow::anyhow!("Command `{}` exited with status: {}", &command, &status))
    }
}

pub fn modprobe(args: &[&str]) -> Result<()> {
    run_command("modprobe", &args).with_context(|| format!("Failed to load module; modprobe arguments: {:?}", &args))?;

    Ok(())
}

pub fn mount_data_partition() -> Result<()> {
    info!("Mounting data partition");
    fs::create_dir_all(&crate::DATA_PART_MOUNTPOINT)?;
    wait_for_file(&crate::DATA_PART);
    Mount::builder().fstype("ext4").data("rw").mount(&crate::DATA_PART, &crate::DATA_PART_MOUNTPOINT)?;

    Ok(())
}

pub fn start_service(service: &str) -> Result<()> {
    run_command("rc-service", &[&service, "start"])?;

    Ok(())
}

pub fn stop_service(service: &str) -> Result<()> {
    run_command("rc-service", &[&service, "stop"])?;

    Ok(())
}

pub fn restart_service(service: &str) -> Result<()> {
    run_command("rc-service", &[&service, "restart"])?;

    Ok(())
}
