use crate::boot_config::BootConfig;
use crate::system::{self, QINIT_BINARIES_DIR_PATH, run_command};

use anyhow::Result;
use log::{debug, info};
use rand::{prelude::*, rng};
use std::sync::{Arc, Mutex};

pub const DEFAULT_WALLPAPER_MODEL: &str = "random";
pub const RANDOM_WALLPAPER_MODEL: &str = DEFAULT_WALLPAPER_MODEL;
pub const DEFAULT_FLOW_PARTICLES_AMOUNT: u64 = 5000;
pub const WALLPAPER_OUT_FILE_PATH: &str = "/tmp/splash_wallpaper.png";
pub const WALLPAPER_MODELS_LIST: &[&str] = &[
    "flow", // 3s
    "clouds", // 5s
    "islands", // 5s
    "lightning", // 1s
    // "nearestpoint", // 12s
    "tangles", // 6s
    // "cellularone", // 30s
    // Too many squares
    "squares", // 0.2s
    // "squareshor",
    // "squaresver",
    // "squaresdiag",
    "squares2", // 0.2s
    // "squares2h",
    // "squares2v",
    // "nearestgradient", // 52s
    // "pattern",
    &RANDOM_WALLPAPER_MODEL,
];
const MAX_GENERATION_RETRIES: u8 = 3;

pub fn generate_wallpaper(boot_config_mutex: &Arc<Mutex<BootConfig>>) -> Result<()> {
    info!("Generating procedural splash wallpaper");

    let mut wallpaper_type = DEFAULT_WALLPAPER_MODEL.to_string();
    {
        let boot_config_mutex = boot_config_mutex.clone();
        let locked_boot_config = boot_config_mutex.lock().unwrap();
        if let Some(wt) = locked_boot_config
            .system
            .splash_wallpaper_options
            .splash_wallpaper
            .clone()
        {
            wallpaper_type = wt;
        }

        if wallpaper_type == RANDOM_WALLPAPER_MODEL {
            let mut rng = rng();
            let available_wallpapers: Vec<&str> = WALLPAPER_MODELS_LIST
                .iter()
                .filter(|&w| *w != RANDOM_WALLPAPER_MODEL)
                .cloned()
                .collect();
            if let Some(selected) = available_wallpapers.choose(&mut rng) {
                wallpaper_type = selected.to_string();
                debug!("Selected random wallpaper type: {}", wallpaper_type);
            } else {
                wallpaper_type = WALLPAPER_MODELS_LIST.first().unwrap().to_string();
                info!("rng failed, going with the first type: {}", &wallpaper_type);
            }
        }
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
                "-s",
                &rand::random::<i32>().unsigned_abs().to_string(),
                "-f",
                &format!("{}", DEFAULT_FLOW_PARTICLES_AMOUNT),
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
