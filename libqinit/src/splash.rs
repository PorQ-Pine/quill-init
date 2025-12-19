use crate::boot_config::BootConfig;
use crate::system::{self, QINIT_BINARIES_DIR_PATH, run_command};
use anyhow::Result;
use log::info;
use std::sync::{Arc, Mutex};

pub const WALLPAPER_OUT_FILE_PATH: &str = "/tmp/splash_wallpaper.png";
const MAX_GENERATION_RETRIES: u8 = 3;

pub fn generate_wallpaper(boot_config_mutex: &Arc<Mutex<BootConfig>>) -> Result<()> {
    info!("Generating procedural splash wallpaper");

    let wallpaper_type;
    {
        let boot_config_mutex = boot_config_mutex.clone();
        let locked_boot_config = boot_config_mutex.lock().unwrap();
        wallpaper_type = locked_boot_config.system.splash_wallpaper.clone();
    }

    system::mount_qinit_binaries()?;

    let mut count = 0;
    while count < MAX_GENERATION_RETRIES {
        // In case something randomly fails within the binary
        if let Err(e) = run_command(
            &format!("{}/procedural_wallpapers", &QINIT_BINARIES_DIR_PATH),
            &[
                "--mode",
                &wallpaper_type,
                "--output",
                &WALLPAPER_OUT_FILE_PATH,
                "-w",
                &format!("{}", crate::SCREEN_W),
                "-h",
                &format!("{}", crate::SCREEN_H),
            ],
        ) {
            if !(count + 1 < MAX_GENERATION_RETRIES) {
                return Err(anyhow::anyhow!("Failed to generate wallpaper: {}", e));
            }
        } else {
            break;
        }
        count += 1;
    }

    Ok(())
}
