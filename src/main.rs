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
mod logging;

use crossterm::event::{self, Event};
use std::time::Duration;
use std::process::exit;
use std::fs;
use base64::engine::general_purpose;
use base64::Engine;
use openssl::pkey::PKey;

const PUBKEY_LOCATION: &str = "/opt/key/public.pem";

fn main() {
    #[cfg(feature = "debug")]
    debug::start_debug_framework();

    // Decoding public key embedded in kernel command line
    let mut cmdline = functions::read_file_string("/proc/cmdline").unwrap_or_else(|e| e);
    let pubkey_base64 = cmdline.split_off(cmdline.len() - 604);
    let pubkey = match general_purpose::STANDARD.decode(pubkey_base64) {
        Ok(pubkey_vector) => {
            fs::write(PUBKEY_LOCATION, &pubkey_vector).expect("Unable to write public key to file");
            pubkey_vector
        }
        Err(e) => {
            logging::info(&format!("Base64 decode error: {e}"), &logging::MessageType::Error);
            return;
        }
    };
    let pubkey_pem = match PKey::public_key_from_pem(&pubkey) {
        Ok(pkey) => pkey,
        Err(e) => {
            logging::info(&format!("Failed to parse PEM public key: {e}"), &logging::MessageType::Error);
            return;
        }
    };

    // Boot info
    let version = functions::read_file_string("/proc/version").unwrap_or_else(|e| e);
    let commit = functions::read_file_string("/.commit").unwrap_or_else(|e| e);

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
}