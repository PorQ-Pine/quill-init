use anyhow::{Context, Result};
use clap::Parser;
use libquillcom::{socket, socket::Command, socket::ErrorDetails};
use postcard::to_allocvec;

// Should be run from the chroot
const QINIT_SOCKET_PATH: &str = "/run/qinit.sock";

// Gemini helped for this ;p
#[derive(Parser)]
#[clap(group(clap::ArgGroup::new("exclusive").required(true).multiple(false)))]
struct ExclusiveOptions {
    #[arg(long, short, group = "exclusive")]
    get_login_credentials: bool,

    #[arg(long, short, group = "exclusive")]
    trigger_fatal_error: bool,
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
    let args = Args::parse();
    let vector;
    if args.exclusive_options.trigger_fatal_error {
        vector = to_allocvec(&socket::ErrorDetails {
            error_reason: args.error_reason,
        })
        .with_context(|| "Failed to create vector with boot command")?;
    } else {
        vector = to_allocvec(&socket::Command::GetLoginCredentials)?;
    }

    socket::write(&args.socket_path, &vector)?;

    Ok(())
}
