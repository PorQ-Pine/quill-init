use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use libqinit::boot_config::BootConfig;
use libqinit::recovery::soft_reset;
use libqinit::system::{
    BootCommand, compress_string_to_xz, get_cmdline_bool, keep_last_lines,
    read_kernel_buffer_singleshot,
};
use log::{error, info};
use qrcode_generator::QrCodeEcc;
use slint::{Image, SharedString, Timer, TimerMode};
use std::fs;
slint::include_modules!();

const TOAST_DURATION_MILLIS: i32 = 5000;
const NOT_AVAILABLE: &str = "(Not currently available)";
const HELP_URI: &str =
    "https://github.com/PorQ-Pine/docs/blob/main/troubleshooting/fatal-errors.md";
const QR_CODE_TAB_INDEX: i32 = 0;
const QR_CODE_NOT_AVAILABLE_TAB_INDEX: i32 = 1;

pub fn setup_gui(
    progress_receiver: Receiver<f32>,
    boot_sender: Sender<BootCommand>,
    interrupt_receiver: Receiver<String>,
    version_string: String,
    short_version_string: String,
    display_progress_bar: bool,
    boot_config_mutex: Arc<Mutex<BootConfig>>,
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
        boot_sender.send(BootCommand::NormalBoot)?;
    }

    // Boot progress bar timer
    let progress_timer = Timer::default();
    progress_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        {
            let gui_weak = gui_weak.clone();
            let boot_sender = boot_sender.clone();
            move || {
                if let Ok(progress) = progress_receiver.try_recv() {
                    if let Some(gui) = gui_weak.upgrade() {
                        if display_progress_bar {
                            /* info!(
                                "Setting boot progress bar's value to {} %",
                                (progress * 100.0) as i32
                            );*/
                            gui.set_boot_progress(progress);
                        }
                        if progress == libqinit::READY_PROGRESS_VALUE {
                            gui.set_startup_finished(true);
                            let _ = boot_sender.send(BootCommand::NormalBoot);
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
                    if gui.get_dialog() == DialogType::Toast {
                        let current_count = gui.get_dialog_millis_count();
                        let future_count = current_count + toast_gc_delay;
                        if future_count > TOAST_DURATION_MILLIS {
                            gui.set_dialog_millis_count(0);
                            gui.set_dialog(DialogType::None);
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
                        let mut program_output = String::new();
                        let mut kernel_buffer = String::new();
                        let qinit_log_file_path =
                            format!("{}/{}", &crate::QINIT_LOG_DIR, &crate::QINIT_LOG_FILE);
                        let lines_to_keep_ui = 150;

                        if let Ok(contents) = fs::read_to_string(&qinit_log_file_path) {
                            program_output = contents.clone();
                            let stripped_program_output =
                                keep_last_lines(&contents, lines_to_keep_ui);
                            gui.set_program_output(SharedString::from(&stripped_program_output));
                        } else {
                            gui.set_program_output(SharedString::from(NOT_AVAILABLE));
                        }

                        if let Ok(contents) = read_kernel_buffer_singleshot() {
                            kernel_buffer = contents.clone();
                            let stripped_kernel_buffer =
                                keep_last_lines(&contents, lines_to_keep_ui);
                            gui.set_kernel_buffer(SharedString::from(&stripped_kernel_buffer));
                        } else {
                            gui.set_kernel_buffer(SharedString::from(NOT_AVAILABLE));
                        }

                        if let Ok(qr_code_svg) = qrcode_generator::to_svg_to_string(
                            &HELP_URI,
                            QrCodeEcc::Low,
                            1024,
                            None::<&str>,
                        ) {
                            if let Ok(help_uri_qr_code) =
                                Image::load_from_svg_data(&qr_code_svg.as_bytes())
                            {
                                gui.set_help_uri_qr_code(help_uri_qr_code);
                            }
                        }

                        // Algorithm to find what number of lines to keep to fit the QR code
                        let mut lines_to_keep_qr = 100;
                        let mut compressed_size = 0;
                        let mut compressed_data = vec![];
                        // Yes, it is very specific: one byte more, and the QR code seems to shrink
                        let ideal_size = 2563;
                        info!("Attempting to optimize QR code data");
                        loop {
                            if compressed_size == 0 || compressed_size >= ideal_size {
                                let mut qr_code_string = String::new();
                                qr_code_string.push_str(&error_reason);
                                qr_code_string.push_str("\n\n");
                                qr_code_string
                                    .push_str(&keep_last_lines(&program_output, lines_to_keep_qr));
                                qr_code_string.push_str("\n\n");
                                qr_code_string
                                    .push_str(&keep_last_lines(&kernel_buffer, lines_to_keep_qr));
                                if let Ok(data) = compress_string_to_xz(&qr_code_string) {
                                    compressed_size = data.len();
                                    if compressed_size <= ideal_size {
                                        info!("Keeping {} lines from each logging source for a total of {} compressed bytes", &lines_to_keep_qr, &compressed_size);
                                        compressed_data = data;
                                        break;
                                    } else {
                                        lines_to_keep_qr -= 1;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }

                        let mut set_not_available = false;
                        if !compressed_data.is_empty() {
                            if let Ok(qr_code_svg) = qrcode_generator::to_svg_to_string(
                                &compressed_data,
                                QrCodeEcc::Low,
                                1024,
                                None::<&str>,
                            ) {
                                if let Ok(debug_qr_code) =
                                    Image::load_from_svg_data(&qr_code_svg.as_bytes())
                                {
                                    gui.set_debug_tab_index(QR_CODE_TAB_INDEX);
                                    gui.set_qr_code_page(QrCodePage::QrCode);
                                    gui.set_debug_qr_code(debug_qr_code);
                                }
                            } else {
                                set_not_available = true;
                            }
                        } else {
                            set_not_available = true;
                        }

                        if set_not_available {
                            gui.set_debug_tab_index(QR_CODE_NOT_AVAILABLE_TAB_INDEX);
                            gui.set_qr_code_page(QrCodePage::NotAvailable);
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
        let boot_sender = boot_sender.clone();
        let gui_weak = gui_weak.clone();
        move || {
            if let Err(e) = boot_sender.send(BootCommand::PowerOff) {
                if let Some(gui) = gui_weak.upgrade() {
                    let err_msg = "Failed to power off";
                    gui.set_dialog_message(SharedString::from(err_msg));
                    gui.set_dialog(DialogType::Toast);
                    error!("{}: {}", &err_msg, e);
                }
            }
        }
    });

    gui.on_reboot({
        let boot_sender = boot_sender.clone();
        let gui_weak = gui_weak.clone();
        move || {
            if let Err(e) = boot_sender.send(BootCommand::Reboot) {
                if let Some(gui) = gui_weak.upgrade() {
                    let err_msg = "Failed to reboot";
                    gui.set_dialog_message(SharedString::from(err_msg));
                    gui.set_dialog(DialogType::Toast);
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

    gui.on_toggle_persistent_rootfs({
        let boot_config_mutex = boot_config_mutex.clone();
        move || {
            let mut locked_boot_config = boot_config_mutex.lock().unwrap();
            locked_boot_config.rootfs.persistent_storage = true;
        }
    });

    gui.on_boot_default({
        let boot_sender = boot_sender.clone();
        let gui_weak = gui_weak.clone();
        move || {
            if let Err(e) = boot_sender.send(BootCommand::NormalBoot) {
                if let Some(gui) = gui_weak.upgrade() {
                    let err_msg = "Failed to send boot command";
                    gui.set_dialog_message(SharedString::from(err_msg));
                    gui.set_dialog(DialogType::Toast);
                    error!("{}: {}", &err_msg, e);
                }
            }
        }
    });

    gui.on_soft_reset({
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                if let Err(e) = soft_reset() {
                    let err_msg = "Failed to soft reset";
                    gui.set_dialog_message(SharedString::from(err_msg));
                    gui.set_dialog(DialogType::Toast);
                    error!("{}: {}", &err_msg, e);
                }
            }
        }
    });

    gui.run()?;

    Ok(())
}
