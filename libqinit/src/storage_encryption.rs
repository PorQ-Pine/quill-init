use anyhow::{Context, Result};
use log::info;
use serde_json;
use std::fs;

use crate::system::run_command;

const GOCRYPTFS_BINARY: &str = "/usr/bin/gocryptfs";
const DISABLED_MODE_FILE: &str = "encryption_disabled";
pub const DISABLED_MODE_PASSWORD: &str = "ENCRYPTION DISABLED";

pub struct UserDetails {
    pub encryption_enabled: bool,
    pub encrypted_key: String,
    pub salt: String,
}

// In Quill OS, either the user sets a password and is forced to use storage encryption, either it does not set a password and cannot use storage encryption.
// Thus, the users in the list returned by this function have all set a password, but some may have chosen to "disable" their encrypted storage for some reason.
// In this case, even if these users will have to login with his password at boot, it might be useful to have left encryption disabled in case it is necessary to retrieve data for recovery purposes and/or if user does not want to lose data in case the account's password has been forgotten.
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
    run_command(
        "/bin/sh",
        &[
            "-c",
            &format!(
                "printf '{}' | {} -allow_other {}/{}/.{} {}/{}/{}",
                &password,
                &GOCRYPTFS_BINARY,
                &crate::MAIN_PART_MOUNTPOINT,
                &crate::SYSTEM_HOME_DIR,
                &user,
                &crate::MAIN_PART_MOUNTPOINT,
                &crate::SYSTEM_HOME_DIR,
                &user
            ),
        ],
    )?;

    Ok(())
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

    let encryption_disabled_file_path = format!(
        "{}/{}/.{}/{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_HOME_DIR,
        &user,
        &DISABLED_MODE_FILE
    );
    if new_password != DISABLED_MODE_PASSWORD && fs::exists(&encryption_disabled_file_path)? {
        fs::remove_file(&encryption_disabled_file_path)?;
    }

    Ok(())
}

pub fn disable(user: &str, password: &str) -> Result<()> {
    change_password(&user, &password, &DISABLED_MODE_PASSWORD)?;
    fs::File::create(&format!(
        "{}/{}/.{}/{}",
        &crate::MAIN_PART_MOUNTPOINT,
        &crate::SYSTEM_HOME_DIR,
        &user,
        &DISABLED_MODE_FILE,
    ))
    .with_context(|| {
        format!(
            "Failed to create file disabling encryption for user '{}'",
            &user
        )
    })?;

    Ok(())
}
