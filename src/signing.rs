use openssl::pkey::Public;
use openssl::pkey::PKey;
use openssl::sign::Verifier;
use openssl::hash::MessageDigest;
use base64::Engine;
use base64::engine::general_purpose;
use anyhow::{Result, Context};
use std::fs;
use log::{info, warn, error};

const PUBKEY_DIR: &str = "/opt/key/";
const PUBKEY_LOCATION: &str = "/opt/key/public.pem";

pub fn decode_public_key_from_cmdline() -> Result<PKey<Public>> {
    let mut cmdline = fs::read_to_string("/proc/cmdline").with_context(|| "Failed to read kernel command line")?; cmdline.pop();
    let pubkey_base64 = cmdline.split_off(cmdline.len() - 604);
    let pubkey_vector = general_purpose::STANDARD.decode(&pubkey_base64).with_context(|| "Failed to decode base64 from kernel command line")?;
    fs::create_dir_all(&PUBKEY_DIR).with_context(|| "Unable to create public key file directory in init ramdisk")?;
    fs::write(&PUBKEY_LOCATION, &pubkey_vector).with_context(|| "Unable to write public key to file")?;
    let pubkey_pem = PKey::public_key_from_pem(&pubkey_vector).with_context(|| "Failed to read public key to PEM format")?;

    Ok(pubkey_pem)
}

pub fn check_signature(pubkey_pem: &PKey<Public>, file: &str, digest_file: &str) -> Result<bool> {
    #[cfg(feature = "free_roam")]
    {
        warn!("Free roam mode: signature of file '{}' was not verified", &file);
        return Ok(true);
    }

    let data = fs::read(&file).with_context(|| format!("Could not read file '{}' for signature verification", &file))?;
    let signature = fs::read(&digest_file).with_context(|| format!("Could not read digest file '{}' for signature verification", &digest_file))?;
    let mut verifier = Verifier::new(MessageDigest::sha256(), &pubkey_pem)?;
    verifier.update(&data)?;
    let pass = verifier.verify(&signature)?;
    if pass {
        info!("File '{}': signature verified successfully", &file);
    } else {
        error!("File '{}': invalid signature", &file);
    }

    Ok(pass)
}
