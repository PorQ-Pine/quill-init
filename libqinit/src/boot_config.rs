use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use std::fs;

const BOOT_CONFIG_FILE: &str = "boot_config.ron";

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
        boot_config.rootfs.persistent_storage = false;
        // Timezone (default to UTC)
        boot_config.system.timezone = "UTC".to_string();

        return boot_config;
    }

    pub fn read() -> Result<BootConfig> {
        let path = Self::get_boot_config_path();
        info!("Attempting to read boot configuration at path '{}'", &path);

        let mut boot_config_to_return = Self::default_boot_config();

        if let Ok(boot_config_str) = fs::read_to_string(&path) {
            if let Ok(boot_config) = ron::from_str::<BootConfig>(&boot_config_str) {
                info!("Found valid boot configuration");
                boot_config_to_return = boot_config;
            } else {
                info!(
                    "Found invalid boot configuration (possibly corrupted or incomplete?): returning default configuration, but enabling 'first_boot_done'"
                );
                boot_config_to_return.flags.first_boot_done = true;
            }
        } else {
            info!("Did not find a valid boot configuration: returning the default one");
        }

        Ok(boot_config_to_return)
    }

    pub fn write(boot_config: &BootConfig) -> Result<()> {
        let path = Self::get_boot_config_path();
        info!("Writing boot configuration at path '{}'", &path);
        fs::write(
            &path,
            ron::ser::to_string_pretty(&boot_config, ron::ser::PrettyConfig::default())?,
        )
        .with_context(|| "Failed to write boot configuration")?;

        Ok(())
    }

    fn get_boot_config_path() -> String {
        return format!("{}/{}", &crate::BOOT_PART_MOUNTPOINT, &BOOT_CONFIG_FILE);
    }
}
