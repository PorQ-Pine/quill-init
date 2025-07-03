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

#[cfg(feature = "debug")]
mod debug;

mod eink;
mod gui;

use crossterm::event::{self, Event};
use std::time::Duration;
use std::process::exit;
use std::fs;
use log::{info, warn, error};
use anyhow::{Context, Result};
use libqinit::system::{mount_data_partition, set_workdir};
use libqinit::signing::{decode_public_key_from_cmdline};

fn main() -> Result<()> {
    env_logger::init();
    #[cfg(not(feature = "gui_only"))]
    {
        // Decode public key embedded in kernel command line
        let pubkey_pem = decode_public_key_from_cmdline()?;

        set_workdir("/")?;
        fs::create_dir_all(&libqinit::DEFAULT_MOUNTPOINT)?;

        // Mount data partition
        mount_data_partition()?;

        #[cfg(feature = "debug")]
        debug::start_debug_framework(&pubkey_pem)?;

        // Boot info
        let mut version = fs::read_to_string("/proc/version").with_context(|| "Failed to read kernel version")?; version.pop();
        let mut commit = fs::read_to_string("/.commit").with_context(|| "Failed to read kernel commit")?; commit.pop();

        eink::load_waveform()?;
        eink::load_modules()?;
        eink::setup_touchscreen()?;

        println!("{}\n\nQuill OS, kernel commit {}\nCopyright (C) 2021-2025 Nicolas Mailloux <nicolecrivain@gmail.com> and Szybet <https://github.com/Szybet>\n", version, commit);
        print!("(initrd) Hit any key to stop auto-boot ... ");

        // Flush stdout to ensure prompt is shown before waiting
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        if event::poll(Duration::from_secs(3)).unwrap() {
            if let Event::Key(_) = event::read().unwrap() {
                exit(0);
            }
        }
        println!();
    }

    gui::setup_gui()?;

    Ok(())
}
