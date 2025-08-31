use anyhow::{Context, Result};
use log::info;

use crate::system::rm_dir_all;

pub fn soft_reset() -> Result<()> {
    info!("Starting soft reset process");

    rm_dir_all(&format!(
        "{}{}{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_DIR,
        &crate::ROOTFS_DIR
    ))
    .with_context(|| "Failed to remove rootfs write cache directory")?;

    Ok(())
}
