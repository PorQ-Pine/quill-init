use std::{
    sync::{Arc, Mutex, mpsc::Receiver},
    thread,
};

use anyhow::{Context, Result};
use core::ops::Deref;
use libquillcom::socket::{self, Command, LoginForm};
use log::info;
use postcard::{from_bytes, to_allocvec};

pub const ROOTFS_SOCKET_PATH: &str = "/overlay/run/qinit_rootfs.sock";

pub fn initialize(login_credentials_receiver: Receiver<LoginForm>) -> Result<()> {
    let login_form_mutex = Arc::new(Mutex::new(LoginForm {
        username: String::new(),
        password: String::new(),
        assumed_valid: false,
    }));
    thread::spawn({
        let login_form_mutex = login_form_mutex.clone();
        move || listen_for_login_credentials(login_credentials_receiver, login_form_mutex)
    });

    thread::spawn({
        let login_form_mutex = login_form_mutex.clone();
        move || listen_for_commands(login_form_mutex.clone())
    });

    Ok(())
}

pub fn listen_for_login_credentials(
    login_credentials_receiver: Receiver<LoginForm>,
    login_form_mutex: Arc<Mutex<LoginForm>>,
) -> Result<()> {
    loop {
        if let Ok(login_form) = login_credentials_receiver.recv() {
            let mut login_form_guard = login_form_mutex.lock().unwrap();
            login_form_guard.username = login_form.username;
            login_form_guard.password = login_form.password;
        }
    }
}

pub fn listen_for_commands(login_form_mutex: Arc<Mutex<LoginForm>>) -> Result<()> {
    info!("Listening for commands");
    let unix_listener = socket::bind(&ROOTFS_SOCKET_PATH)?;
    loop {
        let (unix_stream, _socket_address) = unix_listener.accept()?;
        match from_bytes::<Command>(&socket::read_from_stream(unix_stream)?.deref())? {
            Command::GetLoginCredentials => {
                info!("Sending login credentials to root filesystem");
                let mut login_form_guard = login_form_mutex.lock().unwrap();
                login_form_guard.assumed_valid =
                    !login_form_guard.username.is_empty() && !login_form_guard.password.is_empty();

                let login_form_vec = to_allocvec(&LoginForm {
                    username: login_form_guard.username.clone(),
                    password: login_form_guard.password.clone(),
                    assumed_valid: login_form_guard.assumed_valid,
                })
                .with_context(|| "Failed to create vector with login credentials")?;
                // Write to qoms socket somewhere?
                // socket::write(&ROOTFS_SOCKET_PATH, &login_form_vec)?;
            }
            Command::StopListening => {
                break;
            }
        }
    }

    info!("Stopped listening for commands");
    Ok(())
}
