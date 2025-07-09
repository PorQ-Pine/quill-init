use std::{sync::mpsc::{Sender, Receiver}};

use anyhow::{Context, Result};
use slint::{SharedString, Timer, TimerMode};
use libqinit::system::{get_cmdline_bool, power_off};
use log::{info, warn, error};
slint::include_modules!();

const TOAST_DURATION_MILLIS: i32 = 5000;

pub fn setup_gui(progress_receiver: Receiver<f32>, init_boot_sender: Sender<bool>, version_string: &str) -> Result<()> {
    let gui = AppWindow::new()?;
    let gui_weak = gui.as_weak();

    if get_cmdline_bool("quill_recovery")? {
        info!("Showing QuillBoot menu");
        gui.set_page(Page::QuillBoot);
        gui.set_version_string(SharedString::from(version_string));
    }
    else {
        gui.set_page(Page::BootSplash);
    }

    // Setup boot progress bar timer
    let progress_timer = Timer::default();
    progress_timer.start(TimerMode::Repeated, std::time::Duration::from_millis(100), {
        let gui_weak = gui_weak.clone();
        move || {
            if let Ok(progress) = progress_receiver.try_recv() {
                if let Some(gui) = gui_weak.upgrade() {
                    gui.set_boot_progress(progress);
                }
            }
        }
    });

    // Toasts garbage collector
    // It's not perfect - even though it's probably not noticeable, it doesn't precisely enforce TOAST_DURATION_MILLIS - but considering the small scale of this UI, I think it's more than enough
    let toast_timer = Timer::default();
    let toast_gc_delay = 100;
    toast_timer.start(TimerMode::Repeated, std::time::Duration::from_millis(toast_gc_delay as u64), {
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                if gui.get_dialog() == Dialog::Toast {
                    let current_count = gui.get_dialog_millis_count();
                    let future_count = current_count + toast_gc_delay;
                    if future_count > TOAST_DURATION_MILLIS {
                        gui.set_dialog_millis_count(0);
                        gui.set_dialog(Dialog::None);
                    }
                    else {
                        gui.set_dialog_millis_count(future_count);
                    }
                }
            }
        }
    });

    gui.on_power_off({
        let gui_weak = gui_weak.clone();
        move || {
            if let Err(e) = power_off() {
                if let Some(gui) = gui_weak.upgrade() {
                    let err_msg = "Failed to power off";
                    gui.set_dialog_message(SharedString::from(err_msg));
                    gui.set_dialog(Dialog::Toast);
                    error!("{}: {}", &err_msg, e);
                }
            }
        }
    });

    gui.on_toggle_ui_scale({
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                if gui.get_scaling_factor() == 1 {
                    gui.set_button_scaling_multiplier(0.6);
                    gui.set_scaling_factor(2);
                } else {
                    gui.set_button_scaling_multiplier(1.0);
                    gui.set_scaling_factor(1);
                }
            }
        }
    });

    gui.on_boot_default({
        move || {
            if let Err(e) = init_boot_sender.send(true) {
                if let Some(gui) = gui_weak.upgrade() {
                    let err_msg = "Failed to send boot command";
                    gui.set_dialog_message(SharedString::from(err_msg));
                    gui.set_dialog(Dialog::Toast);
                    error!("{}: {}", &err_msg, e);
                }
            }
        }
    });

    gui.run()?;

    Ok(())
}
