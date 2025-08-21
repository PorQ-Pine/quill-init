use crate::boot_config::BootConfig;
use crate::rootfs;
use anyhow::{Context, Result};
use log::{info, warn};
use rmesg;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
};

const REACHED_TARGET_MAGIC: &str = "systemd[1]: Reached target";
const STARTUP_COMPLETE_MAGIC: &str = "systemd[1]: Startup finished in";

pub fn wait_and_count_targets(
    boot_config: Option<&mut BootConfig>,
    progress_sender: Option<Sender<f32>>,
    boot_finished: Option<Arc<AtomicBool>>,
) -> Result<i32> {
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
    if let Some(boot_config) = boot_config
        && let Some(progress_sender) = progress_sender
    {
        boot_config.rootfs.systemd_targets_total = Some(targets_count);
        progress_sender.send(crate::READY_PROGRESS_VALUE)?;
    }

    if let Some(boot_finished) = boot_finished {
        boot_finished.store(true, Ordering::SeqCst);
    }

    Ok(targets_count)
}

pub fn wait_for_targets(
    boot_config: &mut BootConfig,
    targets_total: i32,
    progress_sender: Sender<f32>,
) -> Result<()> {
    info!("Waiting for systemd 'Reached target' messages to update boot progress bar");
    info!(
        "(Presumed) total number of systemd targets is {}: setting progress bar up accordingly, but recounting targets to check whether or not their number has changed",
        &targets_total
    );

    let boot_finished = Arc::new(AtomicBool::new(false));
    let boot_finished_clone = boot_finished.clone();
    let counting_thread =
        thread::spawn(move || wait_and_count_targets(None, None, Some(boot_finished_clone)));

    let mut targets_count = 0;
    for maybe_entry in rmesg::logs_iter(rmesg::Backend::Default, false, false)? {
        if maybe_entry?.to_string().contains(&REACHED_TARGET_MAGIC) {
            targets_count += 1;
            let progress_value = &rootfs::ROOTFS_MOUNTED_PROGRESS_VALUE
                + (targets_count as f32 / targets_total as f32
                    * (1.0 - &rootfs::ROOTFS_MOUNTED_PROGRESS_VALUE));
            progress_sender.send(progress_value)?;
        }

        if boot_finished.load(Ordering::SeqCst) {
            break;
        }
    }
    info!("Finished waiting for systemd 'Reached target' messages");

    let fresh_targets_count = counting_thread
        .join()
        .map_err(|e| anyhow::anyhow!("Failed to count systemd targets: {:?}", e))??;
    if let Some(old_targets_count) = boot_config.rootfs.systemd_targets_total {
        if fresh_targets_count != old_targets_count {
            info!(
                "Counted different number of systemd targets: {}",
                fresh_targets_count
            );
            boot_config.rootfs.systemd_targets_total = Some(fresh_targets_count);
        }
    }

    progress_sender.send(crate::READY_PROGRESS_VALUE)?;

    Ok(())
}

pub fn get_targets_total(boot_config: &mut BootConfig) -> Result<Option<i32>> {
    let rootfs_file_path = format!(
        "{}/{}/{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_DIR,
        &crate::ROOTFS_FILE
    );
    if fs::exists(&rootfs_file_path)? {
        let current_rootfs_timestamp = fs::metadata(&rootfs_file_path)
            .with_context(|| "Failed to retrieve root filesystem SquashFS archive's metadata")?
            .mtime();
        if current_rootfs_timestamp == boot_config.rootfs.timestamp {
            if let Some(systemd_targets_total) = boot_config.rootfs.systemd_targets_total {
                info!("Displaying boot progress bar");
                return Ok(Some(systemd_targets_total));
            } else {
                warn!(
                    "Could not determine number of systemd targets from boot configuration: not displaying progress bar"
                );
                return Ok(None);
            }
        } else {
            boot_config.rootfs.timestamp = current_rootfs_timestamp;
            info!("Not displaying boot progress bar: number of systemd targets is not yet known");
            return Ok(None);
        }
    } else {
        warn!("Could not find root filesystem SquashFS archive");
        return Ok(None);
    }
}
