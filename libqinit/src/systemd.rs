use crate::boot_config::BootConfig;
use crate::rootfs;
use anyhow::Result;
use log::{info, warn};
use rmesg;
use std::{fs, os::unix::fs::MetadataExt, sync::mpsc::Sender};

const REACHED_TARGET_MAGIC: &str = "systemd[1]: Reached target";
const STARTUP_COMPLETE_MAGIC: &str = "systemd[1]: Startup finished in";

pub fn wait_and_count_targets(boot_config: &mut BootConfig, progress_sender: Sender<f32>) -> Result<()> {
    info!("Waiting for systemd 'Reached target' messages");
    let mut targets_count = 0;
    for maybe_entry in rmesg::logs_iter(rmesg::Backend::Default, false, false)? {
        let entry = maybe_entry?.to_string();
        if entry.contains(&REACHED_TARGET_MAGIC) {
            targets_count += 1;
        } else if entry.contains(&STARTUP_COMPLETE_MAGIC) {
            break;
        }
    }
    info!("Counted {} systemd targets", &targets_count);
    boot_config.systemd_targets_total = Some(targets_count);
    progress_sender.send(crate::READY_PROGRESS_VALUE)?;

    Ok(())
}

pub fn wait_for_targets(targets_total: i32, progress_sender: Sender<f32>) -> Result<()> {
    info!("Waiting for systemd 'Reached target' messages to update boot progress bar");
    info!("Total number of systemd targets is {}", &targets_total);
    let mut targets_count = 0;
    for maybe_entry in rmesg::logs_iter(rmesg::Backend::Default, false, false)? {
        if maybe_entry?.to_string().contains(&REACHED_TARGET_MAGIC) {
            targets_count += 1;
            let progress_value = &rootfs::ROOTFS_MOUNTED_PROGRESS_VALUE
                + (targets_count as f32 / targets_total as f32
                    * (1.0 - &rootfs::ROOTFS_MOUNTED_PROGRESS_VALUE));
            progress_sender.send(progress_value)?;
            if targets_count >= targets_total {
                break;
            }
        }
    }
    info!("Finished waiting for systemd 'Reached target' messages");

    Ok(())
}

pub fn get_targets_total(boot_config: &mut BootConfig) -> Result<Option<i32>> {
    let rootfs_file_path = format!("{}/{}/{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR, &crate::ROOTFS_FILE);
    let current_rootfs_timestamp = fs::metadata(&rootfs_file_path)?.mtime();
    if current_rootfs_timestamp == boot_config.rootfs_timestamp {
        if let Some(systemd_targets_total) = boot_config.systemd_targets_total {
            info!("Displaying boot progress bar");
            return Ok(Some(systemd_targets_total));
        } else {
            warn!(
                "Could not determine number of systemd targets from boot configuration: not displaying progress bar"
            );
            return Ok(None);
        }
    } else {
        boot_config.rootfs_timestamp = current_rootfs_timestamp;
        info!("Not displaying boot progress bar: number of systemd targets is not yet known");
        return Ok(None);
    }
}
