use anyhow::{Context, Result};
use log::info;
use std::fs;
use serde_json;

pub struct UserDetails {
    pub encrypted_key: String,
    pub salt: String,
}

pub fn get_users_using_storage_encryption() -> Result<Vec<String>> {
    info!("Building list of users using storage encryption");
    let users = fs::read_dir(&format!(
        "{}/{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_HOME_DIR
    ))
    .with_context(|| "Failed to read system home directory")?;
    let mut users_using_storage_encryption: Vec<String> = Vec::new();
    for user in users {
        let user = user?;
        if fs::exists(&format!(
            "{}/.using_encryption",
            user.path().to_string_lossy().to_string()
        ))? {
            users_using_storage_encryption.push(user.file_name().to_string_lossy().to_string());
        }
    }
    info!("List is as follows: {:?}", &users_using_storage_encryption);

    Ok(users_using_storage_encryption)
}

pub fn get_encryption_user_details(user: &str) -> Result<UserDetails> {
    let config_path = format!("{}/{}/.{}/gocryptfs.conf", &crate::MAIN_PART_MOUNTPOINT, &crate::SYSTEM_HOME_DIR, &user);
    let json: serde_json::Value = serde_json::from_reader(fs::File::open(&config_path)?)?;
    let not_found = "Not found";
    if let Some(encrypted_key) = json.get("EncryptedKey") && let Some(salt_str) = json["ScryptObject"]["Salt"].as_str() {
        if let Some(encrypted_key_str) = encrypted_key.as_str() {
            return Ok(UserDetails { encrypted_key: encrypted_key_str.to_string(), salt: salt_str.to_string() })
        } else {
            return Ok(UserDetails { encrypted_key: not_found.to_string(), salt: not_found.to_string() })
        }
    } else {
        return Ok(UserDetails { encrypted_key: not_found.to_string(), salt: not_found.to_string() })
    }
}
