use crate::system::{self, QINIT_BINARIES_ARCHIVE, QINIT_BINARIES_DIR_PATH, run_command};
use anyhow::{Context, Result};

pub const WALLPAPER_OUT_FILE_PATH: &str = "/tmp/splash_wallpaper.png";

pub fn generate_wallpaper() -> Result<()> {
    system::mount_qinit_binaries()?;
    run_command(
        &format!("{}/procedural_wallpapers", &QINIT_BINARIES_DIR_PATH),
        &[
            "--mode",
            "flow",
            "--output",
            &WALLPAPER_OUT_FILE_PATH,
            "-w",
            &format!("{}", crate::SCREEN_W),
            "-h",
            &format!("{}", crate::SCREEN_H),
        ],
    )?;

    Ok(())
}
