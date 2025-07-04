use std::sync::mpsc::Receiver;

use anyhow::{Context, Result};
use slint::{LogicalSize, SharedString, Timer, TimerMode};
use libqinit::system::get_cmdline_bool;
use log::{info, warn, error};
slint::include_modules!();

pub fn setup_gui(progress_receiver: Receiver<f32>, kernel_commit: &str) -> Result<()> {
    let gui = AppWindow::new()?;
    let gui_weak = gui.as_weak();

    if get_cmdline_bool("quill_recovery")? {
        gui.set_page(Page::QuillBoot);
        gui.set_version_string(SharedString::from(format!("Kernel commit {}", &kernel_commit)));
    }
    else {
        gui.set_page(Page::BootSplash);
    }

    // Setup boot progress bar timer
    let progress_timer = Timer::default();
    progress_timer.start(TimerMode::Repeated, std::time::Duration::from_millis(100), move || {
        if let Ok(progress) = progress_receiver.try_recv() {
            if let Some(gui) = gui_weak.upgrade() {
                gui.set_boot_progress(progress);
            }
        }
    });

    gui.run()?;

    Ok(())
}
