use anyhow::{Context, Result};
use log::info;

use crate::{boot_config::BootConfig, system::rm_dir_all};

use std::sync::{Arc, Mutex};

pub fn soft_reset(boot_config: Arc<Mutex<BootConfig>>) -> Result<()> {
    info!("Starting soft reset process");

    rm_dir_all(&format!(
        "{}{}{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_DIR,
        &crate::ROOTFS_DIR
    ))
    .with_context(|| "Failed to remove rootfs write cache directory")?;

    rm_dir_all(&format!(
        "{}{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_HOME_DIR
    ))
    .with_context(|| "Failed to remove system home directory")?;

    *boot_config.lock().unwrap() = BootConfig::default_boot_config();

    Ok(())
}
