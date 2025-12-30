use anyhow::{Context, Result};
use log::info;
use serde_json;
use std::{fs, thread};

use crate::system::{bulletproof_unmount, is_mountpoint, run_command};

pub const GOCRYPTFS_BINARY: &str = "/usr/bin/gocryptfs";
pub const DISABLED_MODE_FILE: &str = "encryption_disabled";
pub const DISABLED_MODE_PASSWORD: &str = "ENCRYPTION DISABLED";

pub struct UserDetails {
    pub encryption_enabled: bool,
    pub encrypted_key: String,
    pub salt: String,
}

// In Quill OS, either the user sets a password and is forced to use storage encryption, either it does not set a password and cannot use storage encryption.
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
        if !user.metadata()?.is_dir() {
            continue;
        }
        let user_path = user.path().to_string_lossy().to_string();
        if fs::exists(&format!("{}/gocryptfs.conf", &user_path))? {
            users_using_storage_encryption
                .push(user.file_name().to_string_lossy()[1..].to_string());
        }
    }
    info!("List is as follows: {:?}", &users_using_storage_encryption);

    Ok(users_using_storage_encryption)
}

pub fn get_user_storage_encryption_status(user: &str) -> Result<bool> {
    Ok(!fs::exists(format!(
        "{}/{}/.{}/{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_HOME_DIR,
        &user,
        &DISABLED_MODE_FILE
    ))?)
}

pub fn get_encryption_user_details(user: &str) -> Result<UserDetails> {
    info!("Retrieving encryption user details for '{}'", &user);

    let encryption_enabled = get_user_storage_encryption_status(&user)?;

    let json: serde_json::Value = serde_json::from_reader(
        fs::File::open(format!(
            "{}/{}/.{}/gocryptfs.conf",
            &crate::MAIN_PART_MOUNTPOINT,
            &crate::SYSTEM_HOME_DIR,
            &user
        ))
        .with_context(|| {
            format!(
                "Failed to open gocryptfs configuration file for user '{}'",
                &user
            )
        })?,
    )
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
                encryption_enabled,
                encrypted_key: encrypted_key_str.to_string(),
                salt: salt_str.to_string(),
            });
        } else {
            return Ok(UserDetails {
                encryption_enabled,
                encrypted_key: not_found.to_string(),
                salt: not_found.to_string(),
            });
        }
    } else {
        return Ok(UserDetails {
            encryption_enabled,
            encrypted_key: not_found.to_string(),
            salt: not_found.to_string(),
        });
    }
}

pub fn mount_storage(user: &str, password: &str) -> Result<()> {
    info!("Attempting to mount encrypted storage for user '{}'", &user);
    let home_path_base = format!("{}/{}", &crate::OVERLAY_MOUNTPOINT, &crate::SYSTEM_HOME_DIR);
    let home_path_encrypted = format!("{}/.{}", &home_path_base, &user);
    let home_mountpoint_path = format!("{}/{}", &home_path_base, &user);

    loop {
        if is_mountpoint(&home_path_base)? {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(250));
    }

    if !is_mountpoint(&home_mountpoint_path)? {
        run_command(
            "/bin/sh",
            &[
                "-c",
                &format!(
                    "printf '{}' | {} -allow_other {} {}",
                    &password, &GOCRYPTFS_BINARY, &home_path_encrypted, &home_mountpoint_path,
                ),
            ],
        )?;
    } else {
        return Err(anyhow::anyhow!(
            "User home directory seems to be already mounted"
        ));
    }

    Ok(())
}

pub fn unmount_storage(user: &str) -> Result<()> {
    info!("Unmounting encrypted storage for user '{}'", &user);
    bulletproof_unmount(&format!(
        "{}/{}/{}",
        &crate::OVERLAY_MOUNTPOINT,
        &crate::SYSTEM_HOME_DIR,
        &user
    ))?;

    Ok(())
}
