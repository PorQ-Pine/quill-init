use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use std::fs;

const FLAGS_FILE: &str = "flags.ron";

#[derive(Default, Serialize, Deserialize, PartialEq, Clone)]
pub struct Flags {
    pub first_boot_done: bool,
    pub systemd_targets_total: Option<i32>,
}

impl Flags {
    fn default_flags() -> Flags {
        let mut flags = Flags::default();
        flags.first_boot_done = false;

        return flags;
    }

    pub fn read() -> Result<Flags> {
        let path = Self::get_flags_file_path();
        info!("Attempting to read boot flags file at path '{}'", &path);

        let mut flags_to_return = Self::default_flags();

        if let Ok(flags_str) = fs::read_to_string(&path) {
            if let Ok(flags) = ron::from_str::<Flags>(&flags_str) {
                info!("Found valid boot flags file");
                flags_to_return = flags;
            } else {
                info!(
                    "Found invalid boot flags file (possibly corrupted or incomplete?): returning default flags, but enabling first_boot_done"
                );
                flags_to_return.first_boot_done = true;
            }
        } else {
            info!("Did not find a valid boot flags file: returning default flags");
        }

        Ok(flags_to_return)
    }

    pub fn write(flags: &Flags) -> Result<()> {
        let path = Self::get_flags_file_path();
        info!("Writing boot flags file at path '{}'", &path);
        fs::write(
            &path,
            ron::ser::to_string_pretty(&flags, ron::ser::PrettyConfig::default())?,
        )
        .with_context(|| "Failed to write flags to file")?;

        Ok(())
    }

    fn get_flags_file_path() -> String {
        return format!(
            "{}/{}/{}",
            &crate::DATA_PART_MOUNTPOINT,
            &crate::BOOT_DIR,
            &FLAGS_FILE
        );
    }
}
