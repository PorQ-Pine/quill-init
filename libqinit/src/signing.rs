use openssl::pkey::Public;
use openssl::pkey::PKey;
use openssl::sign::Verifier;
use openssl::hash::MessageDigest;
use anyhow::{Result, Context};
use std::fs;
use log::{info, warn, error};

const PUBKEY_LOCATION: &str = "/opt/key/public.pem";

pub fn read_public_key() -> Result<PKey<Public>> {
    info!("Reading embedded kernel public key");
    let pubkey_bytes = fs::read(&PUBKEY_LOCATION)?;
    let pubkey_pem = PKey::public_key_from_pem(&pubkey_bytes)?;

    Ok(pubkey_pem)
}

pub fn check_signature(pubkey_pem: &PKey<Public>, file: &str, digest_file: &str) -> Result<bool> {
    cfg_if::cfg_if! {
        if #[cfg(feature = "free_roam")] {
            warn!("Free roam mode: signature of file '{}' was not verified", &file);
            return Ok(true);
        } else {
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
    }
}
