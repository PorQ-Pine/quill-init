use std::{
    sync::{Arc, Mutex, mpsc::Receiver},
    thread,
};

use anyhow::{Context, Result};
use core::ops::Deref;
use libquillcom::socket::{self, CommandToQinit, LoginForm};
use log::info;
use postcard::to_allocvec;
use std::io::Write;

pub const ROOTFS_SOCKET_PATH: &str = "/overlay/run/qinit_rootfs.sock";

pub fn initialize(login_credentials_receiver: Receiver<LoginForm>) -> Result<()> {
    let login_form_mutex = Arc::new(Mutex::new(None));
    thread::spawn({
        let login_form_mutex = login_form_mutex.clone();
        move || listen_for_login_credentials(login_credentials_receiver, login_form_mutex)
    });

    thread::spawn({
        let login_form_mutex = login_form_mutex.clone();
        move || listen_for_commands(login_form_mutex)
    });

    Ok(())
}

pub fn listen_for_login_credentials(
    login_credentials_receiver: Receiver<LoginForm>,
    login_form_mutex: Arc<Mutex<Option<LoginForm>>>,
) -> Result<()> {
    loop {
        if let Ok(login_form) = login_credentials_receiver.recv() {
            let mut login_form_guard = login_form_mutex.lock().unwrap();
            if login_form.username.is_empty() || login_form.password.is_empty() {
                *login_form_guard = None;
            } else {
                *login_form_guard = Some(LoginForm { username: login_form.username, password: login_form.password });
            }
        }
    }
}

pub fn listen_for_commands(login_form_mutex: Arc<Mutex<Option<LoginForm>>>) -> Result<()> {
    info!("Listening for commands");
    let unix_listener = socket::bind(&ROOTFS_SOCKET_PATH)?;
    loop {
        let (mut unix_stream, _socket_address) = unix_listener.accept()?;
        match postcard::from_bytes::<CommandToQinit>(&socket::read_from_stream(&unix_stream)?.deref())? {
            CommandToQinit::GetLoginCredentials => {
                info!("Sending login credentials to root filesystem");

                let login_form_guard = login_form_mutex.lock().unwrap().clone();
                let login_form_vec = to_allocvec(&login_form_guard)
                .with_context(|| "Failed to create vector with login credentials")?;

                unix_stream.write_all(&login_form_vec)?;
            }
            CommandToQinit::StopListening => {
                break;
            }
        }
    }

    info!("Stopped listening for commands");
    Ok(())
}
