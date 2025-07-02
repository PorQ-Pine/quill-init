/*
 * quill-init: Initialization program of Quill OS
 * Copyright (C) 2025 Nicolas Mailloux <nicolecrivain@gmail.com>
 * SPDX-License-Identifier: GPL-3.0-only
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

mod system;
mod signing;
mod eink;
mod debug;

use crossterm::event::{self, Event};
use std::time::Duration;
use std::process::exit;
use std::fs;
use std::io;
use log::{info, warn, error};
use anyhow::{Context, Result};

const DATA_PART: &str = "/dev/mmcblk0p6";
const DATA_PART_MOUNTPOINT: &str = "/data/";
const BOOT_DIR: &str = "boot/";
const DEFAULT_MOUNTPOINT: &str = "/mnt/";
const GENERIC_DIGEST_EXT: &str = ".dgst";
const HOME_DIR: &str = "/root/";

fn main() -> Result<()> {
    // Decode public key embedded in kernel command line
    info!("Decoding embedded kernel public key");
    let pubkey_pem = signing::decode_public_key_from_cmdline()?;

    system::set_workdir("/")?;
    fs::create_dir_all(&DEFAULT_MOUNTPOINT)?;

    // Mount data partition
    info!("Mounting data partition");
    system::mount_data_partition()?;

    #[cfg(feature = "debug")]
    debug::start_debug_framework(&pubkey_pem)?;

    // Boot info
    let mut version = fs::read_to_string("/proc/version").with_context(|| "Failed to read kernel version")?; version.pop();
    let mut commit = fs::read_to_string("/.commit").with_context(|| "Failed to read kernel commit")?; commit.pop();

    // Install external libraries which would have been too big for the compressed init ramdisk
    system::install_external_libraries(&pubkey_pem)?;

    // Load waveform from MMC
    eink::load_waveform()?;

    // Load eInk modules
    eink::load_modules()?;

    println!("{}\n\nQuill OS, kernel commit {}\nCopyright (C) 2021-2025 Nicolas Mailloux <nicolecrivain@gmail.com> and Szybet <https://github.com/Szybet>\n", version, commit);

    print!("(initrd) Hit any key to stop auto-boot ... ");
    // Flush stdout to ensure prompt is shown before waiting
    std::io::Write::flush(&mut std::io::stdout()).unwrap();

    if event::poll(Duration::from_secs(5)).unwrap() {
        if let Event::Key(_) = event::read().unwrap() {
            exit(0);
        }
    }
    println!();

    Ok(())
}
