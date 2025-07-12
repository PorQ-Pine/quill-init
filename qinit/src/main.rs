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
use std::thread;
use std::fs;
use log::{info, warn, error};
use anyhow::{Context, Result};
use libqinit::system::{mount_data_partition, mount_rootfs, set_workdir, generate_version_string};
use libqinit::signing::{read_public_key};
use std::sync::mpsc::{channel, Sender, Receiver};

fn main() -> Result<()> {
    env_logger::init();
    // Boot info
    let mut kernel_version = fs::read_to_string("/proc/version").with_context(|| "Failed to read kernel version")?; kernel_version.pop();
    let mut kernel_commit = fs::read_to_string("/.commit").with_context(|| "Failed to read kernel commit")?; kernel_commit.pop();
    let version_string = generate_version_string(&kernel_commit);

    // Decode public key embedded in kernel command line
    let pubkey_pem =  read_public_key()?;

    #[cfg(not(feature = "gui_only"))]
    {
        set_workdir("/")?;
        fs::create_dir_all(&libqinit::DEFAULT_MOUNTPOINT)?;

        // Mount data partition
        mount_data_partition()?;

        // Create boot flags directory
        fs::create_dir_all(format!("{}{}{}", &libqinit::DATA_PART_MOUNTPOINT, &libqinit::BOOT_DIR, &libqinit::FLAGS_DIR))?;

        #[cfg(feature = "debug")]
        debug::start_debug_framework(&pubkey_pem)?;

        eink::load_waveform()?;
        eink::load_modules()?;
        eink::setup_touchscreen()?;

        println!("{}\n\nQuill OS, kernel commit {}\nCopyright (C) 2021-2025 Nicolas Mailloux <nicolecrivain@gmail.com> and Szybet <https://github.com/Szybet>\n", &kernel_version, &kernel_commit);
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

    // Setting up GUI
    let (progress_sender, progress_receiver): (Sender<f32>, Receiver<f32>) = channel();
    let (init_boot_sender, init_boot_receiver): (Sender<bool>, Receiver<bool>) = channel();
    let handle = thread::spawn(move || gui::setup_gui(progress_receiver, init_boot_sender, &version_string));

    // Blocking this function until the main thread receives a signal to continue booting (allowing an user to perform recovery tasks, for example)
    init_boot_receiver.recv()?;

    // Resuming boot
    mount_rootfs(&pubkey_pem)?;
    progress_sender.send(0.1)?;

    // Handling GUI thread issues if there are some
    handle.join().map_err(|e| anyhow::anyhow!("Thread panicked: {:?}", e))??;

    Ok(())
}
