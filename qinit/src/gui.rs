use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use libqinit::boot_config::BootConfig;
use libqinit::recovery::soft_reset;
use libqinit::system::{
    BootCommand, compress_string_to_xz, get_cmdline_bool, keep_last_lines, power_off,
    read_kernel_buffer_singleshot, reboot,
};
use libqinit::wifi;
use log::{error, info};
use qrcode_generator::QrCodeEcc;
use slint::{Image, SharedString, Timer, TimerMode};
use std::{fs, thread};
slint::include_modules!();

pub const TOAST_DURATION_MILLIS: i32 = 5000;
const NOT_AVAILABLE: &str = "(Not currently available)";
const HELP_URI: &str =
    "https://github.com/PorQ-Pine/docs/blob/main/troubleshooting/fatal-errors.md";
const QR_CODE_TAB_INDEX: i32 = 0;
const QR_CODE_NOT_AVAILABLE_TAB_INDEX: i32 = 1;

pub fn setup_gui(
    progress_receiver: Receiver<f32>,
    boot_sender: Sender<BootCommand>,
    interrupt_receiver: Receiver<String>,
    toast_receiver: Receiver<String>,
    version_string: String,
    short_version_string: String,
    display_progress_bar: bool,
    boot_config_mutex: Arc<Mutex<BootConfig>>,
) -> Result<()> {
    let gui = AppWindow::new()?;
    let gui_weak = gui.as_weak();

    // Channels
    let (set_page_sender, set_page_receiver): (Sender<Page>, Receiver<Page>) = channel();
    let (wifi_status_sender, wifi_status_receiver): (Sender<wifi::Status>, Receiver<wifi::Status>) =
        channel();
    let (wifi_command_sender, wifi_command_receiver): (
        Sender<wifi::CommandForm>,
        Receiver<wifi::CommandForm>,
    ) = channel();

    // Guard that ensures that no one can set a page if the current one is Page::Error
    let page_timer = Timer::default();
    page_timer.start(TimerMode::Repeated, std::time::Duration::from_millis(20), {
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                if let Ok(page) = set_page_receiver.try_recv() {
                    info!(
                        "Received request to change current GUI page to '{:?}'",
                        &page
                    );
                    if gui.get_page() == Page::Error {
                        error!("Denying request: current page is '{:?}'", Page::Error);
                    } else {
                        gui.set_page(page);
                    }
                }
            }
        }
    });

    if display_progress_bar {
        gui.set_progress_widget(ProgressWidget::ProgressBar);
    } else {
        gui.set_progress_widget(ProgressWidget::MovingDots);
    }

    if get_cmdline_bool("quill_recovery")? {
        info!("Showing QuillBoot menu");
        set_page_sender.send(Page::QuillBoot)?;
        gui.set_version_string(SharedString::from(version_string));
    } else {
        // Trigger normal boot automatically
        boot_sender.send(BootCommand::NormalBoot)?;
    }

    // Activating switches if needed
    {
        let boot_config_mutex = boot_config_mutex.clone();
        let boot_config_guard = boot_config_mutex.lock().unwrap();

        gui.set_persistent_rootfs(boot_config_guard.rootfs.persistent_storage);
    }

    // Boot progress bar timer
    let progress_timer = Timer::default();
    progress_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        {
            let gui_weak = gui_weak.clone();
            let boot_sender = boot_sender.clone();
            let set_page_sender = set_page_sender.clone();
            move || {
                if let Ok(progress) = progress_receiver.try_recv() {
                    if let Some(gui) = gui_weak.upgrade() {
                        if progress == 0.0 {
                            let _ = set_page_sender.send(Page::BootSplash);
                        }
                        if display_progress_bar {
                            /* info!(
                                "Setting boot progress bar's value to {} %",
                                (progress * 100.0) as i32
                            );*/
                            gui.set_boot_progress(progress);
                        }
                        if progress == libqinit::READY_PROGRESS_VALUE {
                            gui.set_startup_finished(true);
                            let _ = boot_sender.send(BootCommand::BootFinished);
                        }
                    }
                }
            }
        },
    );

    // Timer to show toasts from external threads/classes
    let toast_timer = Timer::default();
    toast_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        {
            let gui_weak = gui_weak.clone();
            move || {
                if let Ok(toast_message) = toast_receiver.try_recv() {
                    if let Some(gui) = gui_weak.upgrade() {
                        toast(&gui, &toast_message);
                    }
                }
            }
        },
    );

    // Toasts garbage collector
    // It's not perfect - even though it's probably not noticeable, it doesn't precisely enforce TOAST_DURATION_MILLIS - but considering the small scale of this UI, I think it's more than enough
    let toast_gc_timer = Timer::default();
    let toast_gc_delay = 100;
    toast_gc_timer.start(
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
            let set_page_sender = set_page_sender.clone();
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
                        let _ = set_page_sender.send(Page::Error);
                    }
                }
            }
        },
    );

    // Wi-Fi
    let wifi_status_timer = Timer::default();
    wifi_status_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        {
            let wifi_command_sender = wifi_command_sender.clone();
            let gui_weak = gui_weak.clone();
            let wifi_disabled_icon =
                Image::load_from_svg_data(include_bytes!("../../icons/wifi-disabled.svg"))?;
            let wifi_not_connected_icon =
                Image::load_from_svg_data(include_bytes!("../../icons/wifi-notconnected.svg"))?;
            let wifi_connected_icon =
                Image::load_from_svg_data(include_bytes!("../../icons/wifi-connected.svg"))?;
            let wifi_error_icon =
                Image::load_from_svg_data(include_bytes!("../../icons/wifi-error.svg"))?;
            let mut hold_wifi_locks = false;
            move || {
                if let Ok(wifi_status) = wifi_status_receiver.try_recv() {
                    info!("Received new Wi-Fi status: {:?}", &wifi_status);
                    if let Some(gui) = gui_weak.upgrade() {
                        match wifi_status.status_type {
                            wifi::StatusType::Disabled => {
                                gui.set_wifi_connected(false);
                                gui.set_wifi_enabled(false);
                                gui.set_wifi_icon(wifi_disabled_icon.to_owned());
                            }
                            wifi::StatusType::NotConnected => {
                                gui.set_wifi_connected(false);
                                gui.set_wifi_enabled(true);
                                gui.set_wifi_icon(wifi_not_connected_icon.to_owned());
                            }
                            wifi::StatusType::Connected => {
                                gui.set_wifi_enabled(true);
                                gui.set_wifi_connected(true);
                                gui.set_wifi_icon(wifi_connected_icon.to_owned());
                            }
                            wifi::StatusType::Error => {
                                gui.set_wifi_connected(false);
                                gui.set_wifi_enabled(true);
                                gui.set_wifi_icon(wifi_error_icon.to_owned());
                                if let Some(error) = wifi_status.error {
                                    toast(&gui, &error);
                                }
                            }
                        }

                        if wifi_status.list.is_none()
                            && wifi_status.status_type != wifi::StatusType::Disabled
                        {
                            // Trigger networks scan
                            if let Err(e) = wifi_command_sender.send(wifi::CommandForm {
                                command_type: wifi::CommandType::GetNetworks,
                                arguments: None,
                            }) {
                                error_toast(&gui, "Failed to get networks list", e.into());
                            }
                            gui.set_wifi_scanning_lock(true);
                            hold_wifi_locks = true;
                        } else {
                            if let Some(networks_list) = wifi_status.list {
                                let mut network_names: Vec<SharedString> = vec![];
                                let mut network_open_vec: Vec<bool> = vec![];
                                for network in networks_list {
                                    network_names.push(SharedString::from(network.name.to_owned()));
                                    network_open_vec.push(network.open);

                                    if network.currently_connected {
                                        info!("Currently connected to network '{}'", &network.name);
                                        gui.set_wifi_connected_name(SharedString::from(
                                            network.name,
                                        ));
                                    } else {
                                        if wifi_status.status_type != wifi::StatusType::Connected {
                                            gui.set_wifi_connected_name(SharedString::new());
                                        }
                                    }
                                }
                                gui.set_wifi_network_names(slint::ModelRc::new(
                                    slint::VecModel::from(network_names),
                                ));
                                gui.set_wifi_network_open_vec(slint::ModelRc::new(
                                    slint::VecModel::from(network_open_vec),
                                ));
                            }
                        }

                        if gui.get_wifi_enabling_lock()
                            && (wifi_status.status_type == wifi::StatusType::NotConnected
                                || wifi_status.status_type == wifi::StatusType::Connected)
                        {
                            gui.set_wifi_enabling_lock(false);
                        }
                        if gui.get_wifi_disabling_lock()
                            && wifi_status.status_type == wifi::StatusType::Disabled
                        {
                            gui.set_wifi_disabling_lock(false);
                        }
                        if !hold_wifi_locks {
                            gui.set_wifi_scanning_lock(false);
                            gui.set_wifi_connecting_lock(false);
                        } else {
                            hold_wifi_locks = false;
                        }
                    }
                }
            }
        },
    );

    thread::spawn(|| wifi::daemon(wifi_status_sender, wifi_command_receiver));
    // Set initial Wi-Fi icon
    wifi_command_sender.send(wifi::CommandForm {
        command_type: wifi::CommandType::GetStatus,
        arguments: None,
    })?;

    gui.on_power_off({
        let boot_sender = boot_sender.clone();
        let gui_weak = gui_weak.clone();
        move || {
            if let Err(e) = boot_sender.send(BootCommand::PowerOff) {
                if let Some(gui) = gui_weak.upgrade() {
                    let mut display_error = true;
                    if gui.get_page() == Page::Error {
                        if let Err(_e) = power_off() {
                            display_error = true;
                        } else {
                            display_error = false;
                        }
                    }

                    if display_error {
                        error_toast(&gui, "Failed to power off", e.into());
                    }
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
                    let mut display_error = true;
                    if gui.get_page() == Page::Error {
                        if let Err(_e) = reboot() {
                            display_error = true;
                        } else {
                            display_error = false;
                        }
                    }

                    if display_error {
                        error_toast(&gui, "Failed to reboot", e.into());
                    }
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
            locked_boot_config.rootfs.persistent_storage =
                !locked_boot_config.rootfs.persistent_storage;
        }
    });

    gui.on_boot_default({
        let boot_sender = boot_sender.clone();
        let gui_weak = gui_weak.clone();
        move || {
            if let Err(e) = boot_sender.send(BootCommand::NormalBoot) {
                if let Some(gui) = gui_weak.upgrade() {
                    error_toast(&gui, "Failed to send boot command", e.into());
                }
            }
        }
    });

    gui.on_soft_reset({
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                if let Err(e) = soft_reset() {
                    error_toast(&gui, "Failed to soft-reset", e.into());
                }
            }
        }
    });

    gui.on_toggle_wifi({
        let wifi_command_sender = wifi_command_sender.clone();
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                if gui.get_wifi_enabled() {
                    gui.set_wifi_disabling_lock(true);
                    if let Err(e) = wifi_command_sender.send(wifi::CommandForm {
                        command_type: wifi::CommandType::Disable,
                        arguments: None,
                    }) {
                        error_toast(&gui, "Failed to enable Wi-Fi", e.into());
                    }
                } else {
                    gui.set_wifi_enabling_lock(true);
                    if let Err(e) = wifi_command_sender.send(wifi::CommandForm {
                        command_type: wifi::CommandType::Enable,
                        arguments: None,
                    }) {
                        error_toast(&gui, "Failed to disable Wi-Fi", e.into());
                    }
                }
            }
        }
    });

    gui.on_connect_to_wifi_network({
        let wifi_command_sender = wifi_command_sender.clone();
        let gui_weak = gui_weak.clone();
        move |network_name, passphrase| {
            if let Some(gui) = gui_weak.upgrade() {
                let err_msg = "Failed to connect to network";
                gui.set_wifi_connecting_lock(true);
                if passphrase.is_empty() {
                    if let Err(e) = wifi_command_sender.send(wifi::CommandForm {
                        command_type: wifi::CommandType::Connect,
                        arguments: Some(wifi::NetworkForm {
                            name: network_name.to_string(),
                            passphrase: None,
                        }),
                    }) {
                        error_toast(&gui, &err_msg, e.into());
                    }
                } else {
                    if let Err(e) = wifi_command_sender.send(wifi::CommandForm {
                        command_type: wifi::CommandType::Connect,
                        arguments: Some(wifi::NetworkForm {
                            name: network_name.to_string(),
                            passphrase: Some(passphrase.to_string()),
                        }),
                    }) {
                        error_toast(&gui, "Failed to connect to network", e.into());
                    }
                }
            }
        }
    });

    gui.on_get_networks({
        let wifi_command_sender = wifi_command_sender.clone();
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                gui.set_wifi_scanning_lock(true);
                if let Err(e) = wifi_command_sender.send(wifi::CommandForm {
                    command_type: wifi::CommandType::GetNetworks,
                    arguments: None,
                }) {
                    error_toast(&gui, "Failed to scan networks", e.into());
                }
            }
        }
    });

    gui.global::<VirtualKeyboardHandler>().on_key_pressed({
        let gui_weak = gui_weak.clone();
        move |key| {
            if let Some(gui) = gui_weak.upgrade() {
                gui.window()
                    .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: key.clone() });
                gui.window()
                    .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: key });
            }
        }
    });

    gui.run()?;

    Ok(())
}

fn toast(gui: &AppWindow, message: &str) {
    gui.set_dialog_message(SharedString::from(message));
    gui.set_dialog(DialogType::Toast);
    info!("{}", &message);
}

fn error_toast(gui: &AppWindow, message: &str, e: anyhow::Error) {
    gui.set_dialog_message(SharedString::from(message));
    gui.set_dialog(DialogType::Toast);
    error!("{}: {}", &message, e);
}
