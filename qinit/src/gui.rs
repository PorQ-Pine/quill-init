use anyhow::{Context, Result};
slint::include_modules!();
use slint::{Weak};
use log::{info, warn, error};

#[macro_export]
macro_rules! gui_upgrade {
    ($gui_weak:expr, $call:ident($($args:expr),*)) => {
        $gui_weak.upgrade_in_event_loop(move |gui| {
            gui.$call($($args),*);
        })?
    };
}

pub fn create_gui() -> Result<AppWindow> {
    info!("Setting up minimal GUI");
    let gui = AppWindow::new()?;

    Ok(gui)
}

pub async fn set_progress(gui_weak: &Weak<AppWindow>, progress: i32) -> Result<()> {
    info!("Setting boot progress bar to value {}", &progress);
    let mut progress_f32 = progress as f32;
    progress_f32 = progress_f32 / 100.0;

    gui_upgrade!(gui_weak, set_boot_progress(progress_f32));

    Ok(())
}
