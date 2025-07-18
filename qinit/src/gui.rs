use std::sync::mpsc::{Receiver, Sender};

use anyhow::{Context, Result};
use libqinit::system::{get_cmdline_bool, power_off};
use log::{error, info, warn};
use slint::{SharedString, Timer, TimerMode};
slint::include_modules!();

const TOAST_DURATION_MILLIS: i32 = 5000;

pub fn setup_gui(
    progress_receiver: Receiver<f32>,
    boot_sender: Sender<bool>,
    version_string: &str,
    display_progress_bar: bool,
) -> Result<()> {
    let gui = AppWindow::new()?;
    let gui_weak = gui.as_weak();

    if get_cmdline_bool("quill_recovery")? {
        info!("Showing QuillBoot menu");
        gui.set_page(Page::QuillBoot);
        gui.set_version_string(SharedString::from(version_string));
    } else {
        if display_progress_bar {
            gui.set_progress_widget(ProgressWidget::ProgressBar);
        } else {
            info!("Showing moving dots animation");
            gui.set_progress_widget(ProgressWidget::MovingDots);
        }
        gui.set_page(Page::BootSplash);
        // Trigger normal boot automatically
        boot_sender.send(true)?;
    }

    // Boot progress bar timer
    let progress_timer = Timer::default();
    progress_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        {
            let gui_weak = gui_weak.clone();
            let boot_sender_clone = boot_sender.clone();
            move || {
                if let Ok(progress) = progress_receiver.try_recv() {
                    if let Some(gui) = gui_weak.upgrade() {
                        if display_progress_bar {
                            info!(
                                "Setting boot progress bar's value to {} %",
                                (progress * 100.0) as i32
                            );
                            gui.set_boot_progress(progress);
                        }
                        if progress == libqinit::READY_PROGRESS_VALUE {
                            gui.set_startup_finished(true);
                            let _ = boot_sender_clone.send(true);
                        }
                    }
                }
            }
        },
    );

    // Toasts garbage collector
    // It's not perfect - even though it's probably not noticeable, it doesn't precisely enforce TOAST_DURATION_MILLIS - but considering the small scale of this UI, I think it's more than enough
    let toast_timer = Timer::default();
    let toast_gc_delay = 100;
    toast_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(toast_gc_delay as u64),
        {
            let gui_weak = gui_weak.clone();
            move || {
                if let Some(gui) = gui_weak.upgrade() {
                    if gui.get_dialog() == Dialog::Toast {
                        let current_count = gui.get_dialog_millis_count();
                        let future_count = current_count + toast_gc_delay;
                        if future_count > TOAST_DURATION_MILLIS {
                            gui.set_dialog_millis_count(0);
                            gui.set_dialog(Dialog::None);
                        } else {
                            gui.set_dialog_millis_count(future_count);
                        }
                    }
                }
            }
        },
    );

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
        let boot_sender_clone = boot_sender.clone();
        move || {
            if let Err(e) = boot_sender_clone.send(true) {
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
