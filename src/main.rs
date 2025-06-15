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

mod logging;

use std::fs::read_to_string;
use crossterm::event::{self, Event};
use std::time::Duration;
use std::process::exit;
use std::fs;
use base64::engine::general_purpose;
use base64::Engine;
use openssl::pkey::PKey;
use openssl::pkey::Public;
use openssl::sign::Verifier;
use openssl::hash::MessageDigest;

const PUBKEY_LOCATION: &str = "/opt/key/public.pem";

fn main() {
    // Decoding public key embedded in kernel command line
    let mut cmdline = read_file_string("/proc/cmdline").unwrap_or_else(|e| e);
    let pubkey_base64 = cmdline.split_off(cmdline.len() - 604);
    let pubkey = match general_purpose::STANDARD.decode(pubkey_base64) {
        Ok(pubkey_vector) => {
            fs::write(PUBKEY_LOCATION, &pubkey_vector).expect("Unable to write public key to file");
            pubkey_vector
        }
        Err(e) => {
            logging::info("Base64 decode error: {e}", &logging::MessageType::Error);
            return;
        }
    };
    let pubkey_pem = match PKey::public_key_from_pem(&pubkey) {
        Ok(pkey) => pkey,
        Err(e) => {
            logging::info("Failed to parse PEM public key: {e}", &logging::MessageType::Error);
            return;
        }
    };

    // Boot info
    let version = read_file_string("/proc/version").unwrap_or_else(|e| e);
    let commit = read_file_string("/.commit").unwrap_or_else(|e| e);

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

fn read_file_string(file_path: &str) -> Result<String, String> {
    let maybe_name = read_to_string(file_path);
    match maybe_name {
        Ok(mut x) => {
            x.pop();
            Ok(x)
        },
        Err(x) => Ok(x.to_string()),
    }
}

fn check_signature(pubkey_pem: &PKey<Public>, file: &str, digest_file: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let data = fs::read(file)?;
    let signature = fs::read(digest_file)?;
    let mut verifier = Verifier::new(MessageDigest::sha256(), &pubkey_pem)?;
    verifier.update(&data)?;
    let pass = verifier.verify(&signature)?;
    Ok(pass)
}

fn start_usbnet_and_telnetd() {
    
}
