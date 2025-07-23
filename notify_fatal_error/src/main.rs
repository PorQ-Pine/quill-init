use libqinit::socket;
use anyhow::{Context, Result};
use postcard::to_allocvec;
use clap::{Parser};

// Should be run from the chroot
const QINIT_SOCKET_PATH: &str = "/run/qinit.sock";

#[derive(Parser)]
#[command(about = "Trigger a fatal error splash")]
struct Args {
    #[arg(long, short, help = "Error reason", default_value = "(No reason provided)")]
    error_reason: String,
    #[arg(long, short, help = "Socket path", default_value = QINIT_SOCKET_PATH)]
    socket_path: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let vector = to_allocvec(&socket::ErrorDetails { error_reason: args.error_reason }).with_context(|| "Failed to create vector with boot command")?;
    socket::write(&args.socket_path, &vector)?;

    Ok(())
}
