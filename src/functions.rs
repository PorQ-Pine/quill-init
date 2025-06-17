use openssl::pkey::Public;
use openssl::sign::Verifier;
use openssl::hash::MessageDigest;
use std::fs::read_to_string;
use openssl::pkey::PKey;
use std::fs;
use std::process::{Command, ExitStatus};
use std::io;

use crate::logging;

pub fn read_file_string(file_path: &str) -> Result<String, String> {
    let maybe_name = read_to_string(file_path);
    match maybe_name {
        Ok(mut x) => {
            x.pop();
            Ok(x)
        },
        Err(x) => Ok(x.to_string()),
    }
}

pub fn check_signature(pubkey_pem: &PKey<Public>, file: &str, digest_file: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let data = fs::read(file)?;
    let signature = fs::read(digest_file)?;
    let mut verifier = Verifier::new(MessageDigest::sha256(), &pubkey_pem)?;
    verifier.update(&data)?;
    let pass = verifier.verify(&signature)?;
    Ok(pass)
}

pub fn run_command_core(command: &str, args: &[&str]) -> io::Result<ExitStatus> {
    Command::new(command)
        .args(args)
        .status()
}

pub fn run_command(command: &str, args: &[&str], context: &str) -> Result<i32, io::Error> {
    match Command::new(command).args(args).status() {
        Ok(status) => {
            let code = status.code().unwrap_or(-1);
            if !status.success() {
                logging::info(
                    &format!("{context}: exited with status {code}"),
                    &logging::MessageType::Error,
                );
            }
            Ok(code)
        }
        Err(e) => {
            logging::info(
                &format!("{context}: failed to execute: {e}"),
                &logging::MessageType::Error,
            );
            Err(e)
        }
    }
}