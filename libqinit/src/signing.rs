use anyhow::{Context, Result};
use log::info;
cfg_if::cfg_if! {
    if #[cfg(feature = "free_roam")] {
        use log::warn;
    } else {
        use openssl::sign::Verifier;
        use openssl::hash::MessageDigest;
        use log::error;
    }
}

use openssl::pkey::PKey;
use openssl::pkey::Public;
use std::fs;

const PUBKEY_PATH: &str = "/opt/key/public.pem";

pub fn read_public_key() -> Result<PKey<Public>> {
    info!("Reading embedded kernel public key");
    let pubkey_bytes =
        fs::read(&PUBKEY_PATH).with_context(|| "Failed to read public key from file")?;
    let pubkey = PKey::public_key_from_pem(&pubkey_bytes)
        .with_context(|| "Failed to read public key from PEM bytes")?;

    Ok(pubkey)
}

pub fn check_signature(_pubkey: &PKey<Public>, file: &str) -> Result<bool> {
    cfg_if::cfg_if! {
        if #[cfg(feature = "free_roam")] {
            warn!("Free roam mode: signature of file '{}' was not verified", &file);
            return Ok(true);
        } else {
            let digest_file = format!("{}{}", &file, &crate::GENERIC_DIGEST_EXT);
            let data = fs::read(&file).with_context(|| format!("Could not read file '{}' for signature verification", &file))?;
            let signature = fs::read(&digest_file).with_context(|| format!("Could not read digest file '{}' for signature verification", &digest_file))?;
            let mut verifier = Verifier::new(MessageDigest::sha256(), &_pubkey)?;
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
