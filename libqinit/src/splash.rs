use crate::boot_config::BootConfig;
use crate::system::{self, QINIT_BINARIES_DIR_PATH, run_command};
use anyhow::Result;
use log::info;
use rand::{prelude::*, rng};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const DEFAULT_WALLPAPER_MODEL: &str = "random";
pub const WALLPAPER_OUT_FILE_PATH: &str = "/tmp/splash_wallpaper.png";
pub const WALLPAPER_MODELS_LIST: &[&str] = &[
    "flow",
    "clouds",
    "islands",
    "lightning",
    "nearestpoint",
    "tangles",
    "cellularone",
    // Too many squares
    "squares",
    // "squareshor",
    // "squaresver",
    // "squaresdiag",
    "squares2",
    // "squares2h",
    // "squares2v",
    "nearestgradient",
    // "pattern", ugly
    "random",
];
const MAX_GENERATION_RETRIES: u8 = 3;
// const hasmaps aren't possible, so we have this
const WALLPAPER_PARTICLES_AMOUNT: &[(&str, u64)] = &[
    ("flow", 5000),
    ("clouds", 3000),
    ("islands", 3000),
    ("lightning", 5000),
    ("nearestpoint", 5000),
    ("tangles", 5000),
    ("cellularone", 1000),
    ("squares", 5000),
    ("squares2", 5000),
    ("nearestgradient", 1000),
    ("random", 5000),
];
const FALLBACK_PARTICLES: u64 = 1000;

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

        if wallpaper_type == "random" {
            let mut rng = rng();
            let available_wallpapers: Vec<&str> = WALLPAPER_MODELS_LIST
                .iter()
                .filter(|&w| *w != "random")
                .cloned()
                .collect();
            if let Some(selected) = available_wallpapers.choose(&mut rng) {
                wallpaper_type = selected.to_string();
                // info!("Selected random wallpaper type: {}", wallpaper_type);
            } else {
                info!("Rng failed somehow, going with the first type.");
                wallpaper_type = WALLPAPER_MODELS_LIST.first().unwrap().to_string();
            }
        }
    }
    let wallpaper_models: HashMap<&'static str, u64> =
    WALLPAPER_PARTICLES_AMOUNT.iter().copied().collect();
    let flow_particles_amount = *wallpaper_models.get(wallpaper_type.as_str()).unwrap_or(&FALLBACK_PARTICLES);

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
                &rand::random::<i32>().to_string(),
                "-f",
                &format!("{}", &flow_particles_amount),
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
