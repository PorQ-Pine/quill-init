use std::sync::mpsc::Receiver;

use anyhow::{Context, Result};
use slint::{Timer, TimerMode, Weak};
slint::include_modules!();

pub fn setup_gui(progress_receiver: Receiver<f32>) -> Result<()> {
    let gui = AppWindow::new()?;
    let gui_weak = gui.as_weak();

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
