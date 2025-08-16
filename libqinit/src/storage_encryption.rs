use anyhow::{Context, Result};
use log::info;
use serde_json;
use std::fs;

use crate::system::run_command;

const GOCRYPTFS_BINARY: &str = "/usr/bin/gocryptfs";
const DISABLED_MODE_PASSWORD: &str = "ENCRYPTION DISABLED";

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
        let user_path = user.path().to_string_lossy().to_string();
        if fs::exists(&format!("{}/gocryptfs.conf", &user_path))?
            && !fs::exists(&format!("{}/encryption_disabled", &user_path))?
        {
            users_using_storage_encryption
                .push(user.file_name().to_string_lossy()[1..].to_string());
        }
    }
    info!("List is as follows: {:?}", &users_using_storage_encryption);

    Ok(users_using_storage_encryption)
}

pub fn get_encryption_user_details(user: &str) -> Result<UserDetails> {
    let config_path = format!(
        "{}/{}/.{}/gocryptfs.conf",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_HOME_DIR,
        &user
    );
    let json: serde_json::Value =
        serde_json::from_reader(fs::File::open(&config_path).with_context(|| {
            format!(
                "Failed to open gocryptfs configuration file for user '{}'",
                &user
            )
        })?)
        .with_context(|| {
            format!(
                "Failed to parse gocryptfs configuration file for user '{}'",
                &user
            )
        })?;
    let not_found = "Not found";
    if let Some(encrypted_key) = json.get("EncryptedKey")
        && let Some(salt_str) = json["ScryptObject"]["Salt"].as_str()
    {
        if let Some(encrypted_key_str) = encrypted_key.as_str() {
            return Ok(UserDetails {
                encrypted_key: encrypted_key_str.to_string(),
                salt: salt_str.to_string(),
            });
        } else {
            return Ok(UserDetails {
                encrypted_key: not_found.to_string(),
                salt: not_found.to_string(),
            });
        }
    } else {
        return Ok(UserDetails {
            encrypted_key: not_found.to_string(),
            salt: not_found.to_string(),
        });
    }
}

pub fn change_password(user: &str, old_password: &str, new_password: &str) -> Result<()> {
    run_command(
        "/bin/sh",
        &[
            "-c",
            &format!(
                "printf '{}\n{}' | {} -passwd {}/{}/.{}",
                &old_password,
                &new_password,
                &GOCRYPTFS_BINARY,
                &crate::MAIN_PART_MOUNTPOINT,
                &crate::SYSTEM_HOME_DIR,
                &user
            ),
        ],
    )
    .with_context(|| {
        format!(
            "Failed to change encrypted storage's password for user '{}'",
            &user
        )
    })?;

    Ok(())
}

pub fn disable(user: &str, password: &str) -> Result<()> {
    change_password(&user, &password, &DISABLED_MODE_PASSWORD)?;
    fs::File::create(&format!(
        "{}/{}/.{}/encryption_disabled",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_HOME_DIR,
        &user
    ))
    .with_context(|| {
        format!(
            "Failed to create file disabling encryption for user '{}'",
            &user
        )
    })?;

    Ok(())
}
