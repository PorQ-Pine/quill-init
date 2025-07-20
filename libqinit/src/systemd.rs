use crate::flags::Flags;
use crate::rootfs;
use crate::system::sha256_match;
use anyhow::Result;
use log::{info, warn};
use rmesg;
use std::sync::mpsc::Sender;

const REACHED_TARGET_MAGIC: &str = "systemd[1]: Reached target";
const STARTUP_COMPLETE_MAGIC: &str = "systemd[1]: Startup finished in";

pub fn wait_and_count_targets(flags: &mut Flags, progress_sender: Sender<f32>) -> Result<()> {
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
    flags.systemd_targets_total = Some(targets_count);
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

pub fn get_targets_total(flags: &mut Flags) -> Result<Option<i32>> {
    if sha256_match(
        &format!(
            "{}/{}/{}",
            &crate::DATA_PART_MOUNTPOINT,
            &crate::BOOT_DIR,
            &crate::ROOTFS_FILE
        ),
        true,
    )? {
        if let Some(systemd_targets_total) = flags.systemd_targets_total {
            info!("Displaying boot progress bar");
            return Ok(Some(systemd_targets_total));
        } else {
            warn!(
                "Could not determine number of systemd targets from flag: not displaying progress bar"
            );
            return Ok(None);
        }
    } else {
        info!("Not displaying boot progress bar: number of systemd targets is not yet known");
        return Ok(None);
    }
}
