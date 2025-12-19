use crate::eink;
use anyhow::{Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs;

const BOOT_CONFIG_FILE: &str = "boot_config.ron";
const DEFAULT_BOOT_CONFIG_SUFFIX: &str = ".new";

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone)]
pub struct BootFlags {
    pub first_boot_done: bool,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone)]
pub struct RootFS {
    pub systemd_targets_total: Option<i32>,
    pub timestamp: i64,
    pub persistent_storage: bool,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone)]
pub struct System {
    pub default_user: Option<String>,
    pub timezone: String,
    // The following option is always enabled by default. If a user chooses to disable it, the "Recovery options" submenu in the GUI will be hidden
    pub recovery_features: bool,
    pub initial_screen_rotation: eink::ScreenRotation,
    pub splash_wallpaper: String,
}

#[cfg(feature = "debug")]
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone)]
pub struct Debug {
    pub usbnet_host_mac_address: Option<String>,
    pub usbnet_dev_mac_address: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone)]
pub struct BootConfig {
    pub flags: BootFlags,
    pub rootfs: RootFS,
    pub system: System,
    #[cfg(feature = "debug")]
    pub debug: Debug,
}

impl BootConfig {
    fn default_boot_config() -> BootConfig {
        let mut boot_config = BootConfig::default();

        // Flags
        boot_config.flags.first_boot_done = false;
        // Root filesystem
        boot_config.rootfs.persistent_storage = true;
        // System
        boot_config.system.timezone = "UTC".to_string();
        boot_config.system.recovery_features = true;
        boot_config.system.splash_wallpaper = "flow".to_string();

        return boot_config;
    }

    pub fn read() -> Result<(BootConfig, bool)> {
        let path = Self::get_boot_config_path(false);
        info!("Attempting to read boot configuration at path '{}'", &path);

        let mut boot_config_to_return = Self::default_boot_config();
        let mut boot_config_valid = false;

        if let Ok(boot_config_str) = fs::read_to_string(&path) {
            if let Ok(boot_config) = ron::from_str::<BootConfig>(&boot_config_str) {
                info!("Found valid boot configuration");
                boot_config_valid = true;
                boot_config_to_return = boot_config;
            } else {
                warn!(
                    "Found invalid boot configuration (possibly corrupted or incomplete?): returning default configuration, but enabling 'first_boot_done'"
                );
                let backup_path = format!("{}.bak", &path);
                info!("Backing old configuration up to path '{}'", &backup_path);
                fs::copy(&path, &backup_path)?;

                boot_config_to_return.flags.first_boot_done = true;

                info!("Writing new boot configuration with defaults");
                Self::write(&boot_config_to_return, true)?;
            }
        } else {
            boot_config_valid = true;
            info!(
                "Could not read boot configuration to string (hint: it might not exist yet). Returning the default one"
            );
            Self::write(&boot_config_to_return, false)?;
        }

        Ok((boot_config_to_return, boot_config_valid))
    }

    pub fn write(boot_config: &BootConfig, slated_for_restoration: bool) -> Result<()> {
        if !slated_for_restoration {
            let default_boot_config_file_to_erase = Self::get_boot_config_path(true);
            if fs::exists(&default_boot_config_file_to_erase)? {
                fs::remove_file(&default_boot_config_file_to_erase)?;
            }
        }

        let path = Self::get_boot_config_path(slated_for_restoration);
        info!("Writing boot configuration at path '{}'", &path);
        fs::write(
            &path,
            ron::ser::to_string_pretty(&boot_config, ron::ser::PrettyConfig::default())?,
        )
        .with_context(|| "Failed to write boot configuration")?;

        Ok(())
    }

    fn get_boot_config_path(slated_for_restoration: bool) -> String {
        let mut path = format!("{}/{}", &crate::BOOT_PART_MOUNTPOINT, &BOOT_CONFIG_FILE);
        if slated_for_restoration {
            path.push_str(&DEFAULT_BOOT_CONFIG_SUFFIX);
        }

        return path;
    }
}
