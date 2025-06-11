use std::fs::read_to_string;
use crossterm::event::{self, Event};
use std::time::Duration;
use std::process::exit;

fn main() {
    let version = read_file("/proc/version").unwrap_or_else(|e| e);
    let mut cmdline = read_file("/proc/cmdline").unwrap_or_else(|e| e);
    let pubkey_base64 = cmdline.split_off(cmdline.len() - 604);
    let commit = read_file("/.commit").unwrap_or_else(|e| e);

    println!("{}\n\nQuill OS, kernel commit {}\nCopyright (C) 2021-2025 Nicolas Mailloux <nicolecrivain@gmail.com> and Szybet <https://github.com/Szybet>\n", version, commit);

    print!("(initrd) Hit any key to stop auto-boot ... ");
    // Flush stdout to ensure prompt is shown before waiting
    std::io::Write::flush(&mut std::io::stdout()).unwrap();

    if event::poll(Duration::from_secs(5)).unwrap() {
        if let Event::Key(_) = event::read().unwrap() {
            exit(0);
        }
    }

    is_rooted_kernel();
}

fn read_file(file_path: &str) -> Result<String, String> {
    let maybe_name = read_to_string(file_path);
    match maybe_name {
        Ok(mut x) => {
            x.pop();
            Ok(x)
        },
        Err(x) => Ok(x.to_string()),
    }
}

fn is_rooted_kernel() -> bool {
   return true 
}

fn start_usbnet_and_telnetd() {
    
}
