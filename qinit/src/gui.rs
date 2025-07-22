use std::sync::mpsc::{Receiver, Sender};

use anyhow::Result;
use libqinit::system::{compress_string_to_xz, get_cmdline_bool, keep_last_lines, power_off, reboot, read_kernel_buffer_singleshot};
use log::{error, info};
use slint::{Image, SharedString, Timer, TimerMode};
use qrcode_generator::QrCodeEcc;
use std::fs;
slint::include_modules!();

const TOAST_DURATION_MILLIS: i32 = 5000;
const NOT_AVAILABLE: &str = "Not currently available";
const HELP_URI: &str = "https://github.com/PorQ-Pine/docs/blob/main/troubleshooting/boot-errors.md";

pub fn setup_gui(
    progress_receiver: Receiver<f32>,
    boot_sender: Sender<bool>,
    interrupt_receiver: Receiver<String>,
    version_string: String,
    short_version_string: String,
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

    let interrupt_timer = Timer::default();
    let interrupt_timer_delay = 100;
    interrupt_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(interrupt_timer_delay),
        {
            let gui_weak = gui_weak.clone();
            move || {
                if let Ok(error_reason) = interrupt_receiver.try_recv() {
                    if let Some(gui) = gui_weak.upgrade() {
                        let mut qr_code_string = String::new();
                        let lines_to_keep = 95;

                        qr_code_string.push_str(&error_reason);
                        qr_code_string.push_str("\n\n");

                        if let Ok(program_output) = fs::read_to_string(&format!(
                            "{}/{}",
                            &crate::QINIT_LOG_DIR,
                            &crate::QINIT_LOG_FILE
                        )) {
                            let stripped_program_output = keep_last_lines(&program_output, lines_to_keep);
                            gui.set_program_output(SharedString::from(&stripped_program_output));
                            qr_code_string.push_str(&stripped_program_output);
                            qr_code_string.push_str("\n\n");
                        } else {
                            gui.set_program_output(SharedString::from(NOT_AVAILABLE));
                        }

                        if let Ok(kernel_buffer) = read_kernel_buffer_singleshot() {
                            let stripped_kernel_buffer = keep_last_lines(&kernel_buffer, lines_to_keep);
                            gui.set_kernel_buffer(SharedString::from(&stripped_kernel_buffer));
                            qr_code_string.push_str(&stripped_kernel_buffer);
                        } else {
                            gui.set_kernel_buffer(SharedString::from(NOT_AVAILABLE));
                        }

                        if let Ok(qr_code_svg) = qrcode_generator::to_svg_to_string(&HELP_URI, QrCodeEcc::Low, 1024, None::<&str>) {
                            if let Ok(help_uri_qr_code) = Image::load_from_svg_data(&qr_code_svg.as_bytes()) {
                                gui.set_help_uri_qr_code(help_uri_qr_code);
                            }
                        }

                        if let Ok(qr_code_svg) = generate_error_splash_qr_code(&qr_code_string) {
                            if let Ok(debug_qr_code) = Image::load_from_svg_data(&qr_code_svg.as_bytes()) {
                                gui.set_debug_qr_code(debug_qr_code);
                            }
                        }

                        gui.set_short_version_string(SharedString::from(&short_version_string));
                        gui.set_error_reason(SharedString::from(&format!("{}", &error_reason)));
                        gui.set_page(Page::Error);
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

    gui.on_reboot({
        let gui_weak = gui_weak.clone();
        move || {
            if let Err(e) = reboot() {
                if let Some(gui) = gui_weak.upgrade() {
                    let err_msg = "Failed to reboot";
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

fn generate_error_splash_qr_code(string: &str) -> Result<String> {
    let compressed_string = compress_string_to_xz(&string)?;
    Ok(qrcode_generator::to_svg_to_string(&compressed_string, QrCodeEcc::Low, 1024, None::<&str>)?)
}
