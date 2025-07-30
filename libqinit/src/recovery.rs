use anyhow::{Context, Result};
use log::info;

pub fn soft_reset() -> Result<()> {
    info!("Starting soft reset process");
    Ok(())
}
