use anyhow::{Context, Result};
use clap::Parser;
use libquillcom::socket;
use postcard::{from_bytes, to_allocvec};
use log::info;

// Should be run from the chroot
const QINIT_SOCKET_PATH: &str = "/run/qinit.sock";

// Gemini helped for this ;p
#[derive(Parser)]
#[clap(group(clap::ArgGroup::new("exclusive").required(true).multiple(false)))]
struct ExclusiveOptions {
    #[arg(long, short, group = "exclusive")]
    get_login_credentials: bool,

    #[arg(long, short = 'f', group = "exclusive")]
    trigger_fatal_error: bool,

    #[arg(long, short = 'p', group = "exclusive")]
    trigger_poweroff_splash: bool,

    #[arg(long, short = 'l', group = "exclusive")]
    trigger_login_page_switch: bool,
}

#[derive(Parser)]
#[command(about = "Test qinit socket(s)")]
struct Args {
    #[clap(flatten)]
    exclusive_options: ExclusiveOptions,
    #[arg(
        long,
        short,
        requires("trigger_fatal_error"),
        help = "Error reason",
        default_value = "(No reason provided)"
    )]
    error_reason: String,
    #[arg(long, short, help = "Socket path", default_value = QINIT_SOCKET_PATH)]
    socket_path: String,
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    let vector;
    if args.exclusive_options.trigger_fatal_error {
        vector = to_allocvec(&socket::ErrorDetails {
            error_reason: args.error_reason,
        })
        .with_context(|| "Failed to create vector with boot command")?;
    } else if args.exclusive_options.get_login_credentials {
        vector = to_allocvec(&socket::CommandToQinit::GetLoginCredentials)?;
    } else if args.exclusive_options.trigger_login_page_switch {
        vector = to_allocvec(&socket::CommandToQinit::TriggerSwitchToLoginPage)?;
    } else {
        vector = to_allocvec(&socket::CommandToQinit::TriggerSplash(socket::PrimitiveShutDownType::PowerOff))?;
    }

    let mut reply = Vec::new();
    if args.exclusive_options.get_login_credentials || args.exclusive_options.trigger_poweroff_splash {
        reply = socket::write_and_read(&args.socket_path, &vector)?;
    } else {
        socket::write(&args.socket_path, &vector)?;
    }

    if args.exclusive_options.get_login_credentials {
        let credentials = &from_bytes::<Option<socket::LoginForm>>(&reply)?;
        if let Some(c) = credentials {
            info!("Username: '{}'", c.username);
            info!("Password: '{}'", c.password);
        }
    }

    Ok(())
}
