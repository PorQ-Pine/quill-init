use openssl::pkey::Public;
use openssl::sign::Verifier;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use std::{fs, process::Command, thread, time::Duration};
use log::{info, warn, error};
use anyhow::{Context, Result, Error};

pub fn check_signature(pubkey_pem: &PKey<Public>, file: &str, digest_file: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let data = fs::read(file)?;
    let signature = fs::read(digest_file)?;
    let mut verifier = Verifier::new(MessageDigest::sha256(), &pubkey_pem)?;
    verifier.update(&data)?;
    let pass = verifier.verify(&signature)?;
    Ok(pass)
}

pub fn run_command(command: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(command)
        .args(args)
        .status()
        .with_context(|| format!("Failed to execute command: {command}"))?;

    if status.success() {
        Ok(())
    } else {
        return Err(anyhow::anyhow!("Command `{command}` exited with status: {status}"))
    }
}

pub fn wait_for_file(file: &str) {
    while !fs::metadata(file).is_ok() {
        thread::sleep(Duration::from_millis(100));
    }
}
