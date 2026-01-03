use anyhow::{Context, Result};
use core::ops::Deref;
use libquillcom::socket::{self, AnswerFromQinit, CommandToQinit, LoginForm};
use log::{debug, info};
use postcard::to_allocvec;
use socket::PrimitiveShutDownType;
use std::io::Write;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
    },
    thread,
};

pub const ROOTFS_SOCKET_PATH: &str = "/overlay/run/qinit_rootfs.sock";

pub fn initialize(
    login_credentials_receiver: Receiver<LoginForm>,
    splash_sender: Sender<PrimitiveShutDownType>,
    splash_ready_receiver: Receiver<()>,
    can_shut_down: Arc<AtomicBool>,
    login_page_trigger_sender: Sender<()>,
) -> Result<()> {
    let login_form_mutex = Arc::new(Mutex::new(None));
    thread::spawn({
        let login_form_mutex = login_form_mutex.clone();
        move || listen_for_login_credentials(login_credentials_receiver, login_form_mutex)
    });

    thread::spawn({
        let login_form_mutex = login_form_mutex.clone();
        move || {
            listen_for_commands(
                login_form_mutex,
                splash_sender,
                splash_ready_receiver,
                can_shut_down,
                login_page_trigger_sender,
            )
        }
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
                *login_form_guard = Some(LoginForm {
                    username: login_form.username,
                    password: login_form.password,
                });
            }
        }
    }
}

pub fn listen_for_commands(
    login_form_mutex: Arc<Mutex<Option<LoginForm>>>,
    splash_sender: Sender<PrimitiveShutDownType>,
    splash_ready_receiver: Receiver<()>,
    can_shut_down: Arc<AtomicBool>,
    login_page_trigger_sender: Sender<()>,
) -> Result<()> {
    info!("Listening for commands");
    let unix_listener = socket::bind(&ROOTFS_SOCKET_PATH)?;
    loop {
        let (mut unix_stream, _socket_address) = unix_listener.accept()?;
        match postcard::from_bytes::<CommandToQinit>(
            &socket::read_from_stream(&unix_stream)?.deref(),
        )? {
            CommandToQinit::GetLoginCredentials => {
                debug!("Sending login credentials to root filesystem");

                let login_form_guard = login_form_mutex.lock().unwrap().clone();
                let login_form_vec = to_allocvec(&AnswerFromQinit::Login(login_form_guard))
                    .with_context(|| "Failed to create vector with login credentials")?;

                unix_stream
                    .write_all(&login_form_vec)
                    .with_context(|| "Failed to send login credentials")?;
            }
            CommandToQinit::TriggerSplash(shut_down_type) => {
                info!(
                    "Displaying splash screen for shut down type '{:?}'",
                    shut_down_type
                );
                splash_sender
                    .send(shut_down_type)
                    .with_context(|| "Failed to send splash type from socket call")?;
                splash_ready_receiver
                    .recv()
                    .with_context(|| "Failed to receive message from splash readiness sender")?;

                loop {
                    if can_shut_down.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(100));
                }

                let reply = to_allocvec(&AnswerFromQinit::SplashReady)?;
                unix_stream
                    .write_all(&reply)
                    .with_context(|| "Failed to send splash readiness status")?;
            }
            CommandToQinit::TriggerSwitchToLoginPage => {
                let _ = login_page_trigger_sender.send(());
            }
            CommandToQinit::StopListening => {
                break;
            }
        }
    }

    info!("Stopped listening for commands");
    Ok(())
}
