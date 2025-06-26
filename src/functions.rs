use openssl::pkey::Public;
use openssl::sign::Verifier;
use openssl::hash::MessageDigest;
use std::fs::read_to_string;
use openssl::pkey::PKey;
use std::fs;
use std::process::{Command, ExitStatus};
use std::io::{self, Error, ErrorKind};
use log::{info, warn, error};

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

pub fn run_command(command: &str, args: &[&str], context: &str) -> Result<(), io::Error> {
    match Command::new(command).args(args).status() {
        Ok(status) => {
            if status.success() {
                Ok(())
            }
            else {
                let msg = format!("{context}: command exited with status {status}");
                error!("{context}");
                Err(Error::new(ErrorKind::Other, msg))
            }
        }
        Err(e) => {
            error!("{context}: failed to execute: {e}");
            Err(e)
        }
    }
}