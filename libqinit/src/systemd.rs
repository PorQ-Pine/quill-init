use std::{sync::mpsc::{Sender}};
use anyhow::Result;
use rmesg;
use crate::rootfs;
use crate::system::{sha256_match};
use crate::flag::{self, Flag};
use log::{info};

const REACHED_TARGET_MAGIC: &str = "systemd[1]: Reached target";
const STARTUP_COMPLETE_MAGIC: &str = "systemd[1]: Startup finished in";

pub fn wait_and_count_targets() -> Result<()> {
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
    flag::write_string(Flag::SYSTEMD_TARGETS_TOTAL, &targets_count.to_string())?;

    Ok(())
}

pub fn wait_for_targets(progress_sender: Sender<f32>) -> Result<()> {
    info!("Waiting for systemd 'Reached target' messages to update boot progress bar");
    let targets_total = flag::read_string(Flag::SYSTEMD_TARGETS_TOTAL)?.parse::<i32>()?;
    info!("Total number of systemd targets is {}", &targets_total);
    let mut targets_count = 0;
    for maybe_entry in rmesg::logs_iter(rmesg::Backend::Default, false, false)? {
        if maybe_entry?.to_string().contains(&REACHED_TARGET_MAGIC) {
            targets_count += 1;
            let progress_value = &rootfs::ROOTFS_MOUNTED_PROGRESS_VALUE + (targets_count as f32 / targets_total as f32 * (1.0 - &rootfs::ROOTFS_MOUNTED_PROGRESS_VALUE));
            progress_sender.send(progress_value)?;
            if targets_count >= targets_total {
                break;
            }
        }
    }
    info!("Finished waiting for systemd 'Reached target' messages");

    Ok(())
}

pub fn can_display_boot_progress_bar() -> Result<bool> {
    if sha256_match(&format!("{}/{}/{}", &crate::DATA_PART_MOUNTPOINT, &crate::BOOT_DIR, &crate::ROOTFS_FILE), true)? && flag::is_set(Flag::SYSTEMD_TARGETS_TOTAL)? {
        info!("Displaying boot progress bar");
        return Ok(true)
    } else {
        info!("Not displaying boot progress bar: number of systemd targets is not yet known");
        return Ok(false)
    }
}
