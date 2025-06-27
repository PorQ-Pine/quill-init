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

mod functions;
mod debug;

use crossterm::event::{self, Event};
use std::time::Duration;
use std::process::exit;
use std::fs;
use std::io;
use base64::engine::general_purpose;
use base64::Engine;
use openssl::pkey::PKey;
use log::{info, warn, error};
use sys_mount::Mount;
use anyhow::{Context, Result};

const OS_PART: &str = "/dev/mmcblk0p6";
const OS_PART_MOUNTPOINT: &str = "/boot/";
const WAVEFORM_PART: &str = "/dev/mmcblk0p2";
const WAVEFORM_FILE: &str = "ebc.wbf";
const WAVEFORM_DIR: &str = "/usr/lib/firmware/rockchip/";
const PUBKEY_DIR: &str = "/opt/key/";
const PUBKEY_LOCATION: &str = "/opt/key/public.pem";

fn main() -> Result<()> {
    // Mounting boot partition
    info!("Mounting boot partition");
    fs::create_dir_all(crate::OS_PART_MOUNTPOINT)?;
    functions::wait_for_file(OS_PART);
    Mount::builder().fstype("ext4").data("rw").mount(crate::OS_PART, crate::OS_PART_MOUNTPOINT)?;
    #[cfg(feature = "debug")]
    debug::start_debug_framework()?;

    // Decoding public key embedded in kernel command line
    info!("Decoding embedded kernel public key");
    let mut cmdline = fs::read_to_string("/proc/cmdline").with_context(|| "Failed to read kernel command line")?; cmdline.pop();
    let pubkey_base64 = cmdline.split_off(cmdline.len() - 604);
    let pubkey_vector = general_purpose::STANDARD.decode(pubkey_base64).with_context(|| "Failed to decode base64 from kernel command line")?;
    fs::create_dir_all(PUBKEY_DIR).with_context(|| "Unable to create public key file directory in init ramdisk")?;
    fs::write(PUBKEY_LOCATION, &pubkey_vector).with_context(|| "Unable to write public key to file")?;
    let pubkey_pem = PKey::public_key_from_pem(&pubkey_vector).with_context(|| "Failed to read public key to PEM format")?;

    // Boot info
    let mut version = fs::read_to_string("/proc/version").with_context(|| "Failed to read kernel version")?; version.pop();
    let mut commit = fs::read_to_string("/.commit").with_context(|| "Failed to read kernel commit")?; commit.pop();

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

    // Loading waveform from MMC
    info!("Loading waveform from MMC");
    let waveform_path = WAVEFORM_DIR.to_owned() + WAVEFORM_FILE;
    let waveform = fs::read(&WAVEFORM_PART).with_context(|| "Failed to read eInk waveform")?;
    fs::create_dir_all(&WAVEFORM_DIR).with_context(|| "Failed to create waveform's directory")?;
    fs::write(waveform_path, &waveform).with_context(|| "Failed to write waveform to file")?;

    Ok(())
}
