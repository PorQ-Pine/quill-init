use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Result;
use chrono::prelude::*;
use libqinit::boot_config::BootConfig;
use libqinit::brightness;
use libqinit::eink::{self, ScreenRotation};
use libqinit::networking;
use libqinit::recovery::soft_reset;
use libqinit::splash;
use libqinit::storage_encryption;
use libqinit::system::{
    BootCommand, BootCommandForm, PowerDownMode, compress_string_to_xz, get_cmdline_bool,
    keep_last_lines, read_kernel_buffer_singleshot, shut_down,
};
use libqinit::wifi;
use libqinit::{battery, system};
use libquillcom::socket::{LoginForm, PrimitiveShutDownType};
use log::{debug, error, info};
use qrcode_generator::QrCodeEcc;
use slint::{Image, SharedString, Timer, TimerMode};
use std::{fs, path::Path, thread};
slint::include_modules!();

pub const TOAST_DURATION_MILLIS: i32 = 5000;
const NOT_AVAILABLE: &str = "(Not currently available)";
const HELP_URI: &str =
    "https://github.com/PorQ-Pine/docs/blob/main/troubleshooting/fatal-errors.md";
const QR_CODE_TAB_INDEX: i32 = 0;
const QR_CODE_NOT_AVAILABLE_TAB_INDEX: i32 = 1;

pub fn setup_gui(
    progress_receiver: Receiver<f32>,
    boot_sender: Sender<BootCommandForm>,
    login_credentials_sender: Sender<LoginForm>,
    splash_receiver: Receiver<PrimitiveShutDownType>,
    splash_ready_sender: Sender<()>,
    interrupt_receiver: Receiver<String>,
    toast_sender: Sender<String>,
    toast_receiver: Receiver<String>,
    version_string: String,
    short_version_string: String,
    display_progress_bar: bool,
    boot_config_mutex: Arc<Mutex<BootConfig>>,
    boot_config_valid: bool,
) -> Result<()> {
    let gui = AppWindow::new()?;
    let gui_weak = gui.as_weak();
    let first_boot_done;
    let can_shut_down = Arc::new(AtomicBool::new(false));
    let core_settings_finished_running = Arc::new(AtomicBool::new(false));
    let (core_settings_sender, core_settings_receiver): (Sender<()>, Receiver<()>) = channel();

    // Boot configuration
    set_default_user_from_boot_config(&gui, boot_config_mutex.clone());
    {
        let boot_config_mutex = boot_config_mutex.clone();
        let boot_config_guard = boot_config_mutex.lock().unwrap();

        first_boot_done = boot_config_guard.flags.first_boot_done;

        // Activate switches if needed
        gui.set_persistent_rootfs(boot_config_guard.rootfs.persistent_storage);
        gui.set_recovery_features(boot_config_guard.system.recovery_features);
        match boot_config_guard.system.initial_screen_rotation {
            ScreenRotation::Cw0 => gui.set_orientations_list_index(0),
            ScreenRotation::Cw90 => gui.set_orientations_list_index(1),
            ScreenRotation::Cw180 => gui.set_orientations_list_index(2),
            ScreenRotation::Cw270 => gui.set_orientations_list_index(3),
        }

        // Splash wallpaper settings
        let splash_wallpapers_models_vec: Vec<SharedString> = splash::WALLPAPER_MODELS_LIST
            .iter()
            .map(|user| SharedString::from(*user))
            .collect();
        gui.set_splash_wallpaper_models_list(slint::ModelRc::new(slint::VecModel::from(
            splash_wallpapers_models_vec,
        )));

        if let Some(splash_wallpaper_model) = &boot_config_guard
            .system
            .splash_wallpaper_options
            .splash_wallpaper
        {
            // ChatGPT did help here...
            let index = splash::WALLPAPER_MODELS_LIST
                .iter()
                .position(|&name| name == splash_wallpaper_model)
                .map(|i| i as i32);
            if let Some(i) = index {
                gui.set_splash_wallpaper_models_list_index(i);
            }
        }
    }

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
                        if page == Page::UserLogin {
                            gui.set_login_captive_portal(true);
                        }
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

    gui.set_version_string(SharedString::from(version_string));

    let quill_recovery = get_cmdline_bool("quill_recovery")?;
    gui.set_quill_recovery(quill_recovery);

    if !boot_config_valid {
        set_page_sender.send(Page::InvalidBootConfig)?;
    }

    if boot_config_valid && quill_recovery {
        info!("Showing QuillBoot menu");
        thread::spawn(|| {
            brightness::set_brightness_unified(
                &libqinit::brightness::MAX_BRIGHTNESS / 2 as i32,
                &libqinit::brightness::MAX_BRIGHTNESS / 2 as i32,
            )
        });
        set_page_sender.send(Page::QuillBoot)?;
    } else if boot_config_valid {
        // Trigger normal boot automatically
        let login_credentials_sender = login_credentials_sender.clone();
        let core_settings_sender = core_settings_sender.clone();
        boot_normal(
            &gui,
            &boot_sender,
            &set_page_sender,
            &gui.get_default_user().to_string(),
            first_boot_done,
            login_credentials_sender,
            core_settings_sender,
        )?;
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
            let can_shut_down = can_shut_down.clone();
            move || {
                if let Ok(progress) = progress_receiver.try_recv() {
                    if let Some(gui) = gui_weak.upgrade() {
                        if progress == 0.0 && !gui.get_login_captive_portal() {
                            let _ = set_page_sender.send(Page::BootSplash);
                        }
                        if display_progress_bar {
                            debug!(
                                "Setting boot progress bar's value to {} %",
                                (progress * 100.0) as i32
                            );
                            gui.set_boot_progress(progress);
                        }
                        if progress == libqinit::READY_PROGRESS_VALUE {
                            gui.set_startup_finished(true);
                            let command: BootCommand;
                            match gui.get_shutdown_command() {
                                RootFsShutDownCommand::PowerOff => {
                                    command = BootCommand::PowerOffRootFS;
                                }
                                RootFsShutDownCommand::Reboot => {
                                    command = BootCommand::RebootRootFS;
                                }
                                RootFsShutDownCommand::None => command = BootCommand::BootFinished,
                            };
                            let _ = boot_sender.send(BootCommandForm {
                                command: command,
                                can_shut_down: Some(can_shut_down.clone()),
                            });
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
                        if !gui.get_sticky_toast() {
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
            }
        },
    );

    let splash_timer = Timer::default();
    splash_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        {
            let gui_weak = gui_weak.clone();
            let set_page_sender = set_page_sender.clone();
            let can_shut_down = can_shut_down.clone();
            move || {
                if let Ok(shut_down_type) = splash_receiver.try_recv() {
                    if let Some(gui) = gui_weak.upgrade() {
                        set_wallpaper_splash_text(&gui, &shut_down_type);
                        if shut_down_type == PrimitiveShutDownType::PowerOff {
                            gui.invoke_generate_splash_wallpaper(true);
                        } else {
                            handle_screen_refresh(true, can_shut_down.clone());
                        }
                        let _ = set_page_sender.send(Page::ShutDownSplash);
                    }
                }
            }
        },
    );

    // Time display timer
    let time_display_timer = Timer::default();
    time_display_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(500),
        {
            let gui_weak = gui_weak.clone();
            move || {
                if let Some(gui) = gui_weak.upgrade() {
                    let current_time: DateTime<Local> = Local::now();
                    gui.set_current_time(SharedString::from(
                        current_time.format("%H : %M").to_string(),
                    ));
                }
            }
        },
    );

    // Fatal errors
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
                        // Yes, it is very specific: one more byte, and the QR code seems to shrink
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
                                if let Ok(ip_address) =
                                    networking::get_if_ip_address(&wifi::WIFI_IF)
                                {
                                    gui.set_wifi_ip_address(SharedString::from(&ip_address));
                                }
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

    // System
    gui.on_power_off({
        let boot_sender = boot_sender.clone();
        let gui_weak = gui_weak.clone();
        let can_shut_down = can_shut_down.clone();
        move || {
            let shut_down_type = PrimitiveShutDownType::PowerOff;
            if let Some(gui) = gui_weak.upgrade() {
                set_wallpaper_splash_text(&gui, &shut_down_type);
                if let Err(e) = boot_sender.send(BootCommandForm {
                    command: BootCommand::PowerOff,
                    can_shut_down: Some(can_shut_down.clone()),
                }) {
                    let display_error;
                    if let Err(_e) = gui_shut_down(
                        &gui,
                        shut_down_type,
                        PowerDownMode::Normal,
                        can_shut_down.clone(),
                    ) {
                        display_error = true;
                    } else {
                        display_error = false;
                    }

                    if display_error {
                        error_toast(&gui, "Failed to power off", e.into());
                    }
                }
            }
        }
    });

    gui.on_direct_power_off({
        let gui_weak = gui_weak.clone();
        let can_shut_down = can_shut_down.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                let power_down_mode = determine_power_down_mode(&gui);
                if let Err(e) = gui_shut_down(
                    &gui,
                    PrimitiveShutDownType::PowerOff,
                    power_down_mode,
                    can_shut_down.clone(),
                ) {
                    error_toast(&gui, "Failed to power off", e.into());
                }
            }
        }
    });

    gui.on_reboot({
        let boot_sender = boot_sender.clone();
        let can_shut_down = can_shut_down.clone();
        let gui_weak = gui_weak.clone();
        move || {
            let shut_down_type = PrimitiveShutDownType::Reboot;
            if let Some(gui) = gui_weak.upgrade() {
                set_wallpaper_splash_text(&gui, &shut_down_type);
                if let Err(e) = boot_sender.send(BootCommandForm {
                    command: BootCommand::Reboot,
                    can_shut_down: Some(can_shut_down.clone()),
                }) {
                    let display_error;
                    if let Err(_e) = gui_shut_down(
                        &gui,
                        shut_down_type,
                        PowerDownMode::Normal,
                        can_shut_down.clone(),
                    ) {
                        display_error = true;
                    } else {
                        display_error = false;
                    }

                    if display_error {
                        error_toast(&gui, "Failed to reboot", e.into());
                    }
                }
            }
        }
    });

    gui.on_direct_reboot({
        let can_shut_down = can_shut_down.clone();
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                let power_down_mode = determine_power_down_mode(&gui);
                if let Err(e) = gui_shut_down(
                    &gui,
                    PrimitiveShutDownType::Reboot,
                    power_down_mode,
                    can_shut_down.clone(),
                ) {
                    error_toast(&gui, "Failed to reboot", e.into());
                }
            }
        }
    });

    // Scaling
    gui.on_toggle_ui_scale({
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                if gui.get_scaling_factor() == 1.0 {
                    gui.set_button_scaling_multiplier(0.6);
                    gui.set_scaling_factor(1.25);
                } else {
                    gui.set_button_scaling_multiplier(1.0);
                    gui.set_scaling_factor(1.0);
                }
            }
        }
    });

    // Boot configuration
    gui.on_toggle_persistent_rootfs({
        let boot_config_mutex = boot_config_mutex.clone();
        move || {
            let mut locked_boot_config = boot_config_mutex.lock().unwrap();
            locked_boot_config.rootfs.persistent_storage =
                !locked_boot_config.rootfs.persistent_storage;
        }
    });

    // System commands
    gui.on_boot_default({
        let boot_sender = boot_sender.clone();
        let set_page_sender = set_page_sender.clone();
        let wifi_command_sender = wifi_command_sender.clone();
        let login_credentials_sender = login_credentials_sender.clone();
        let core_settings_sender = core_settings_sender.clone();
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                // Turn off Wi-Fi
                if let Err(e) = wifi_command_sender.send(wifi::CommandForm {
                    command_type: wifi::CommandType::Disable,
                    arguments: None,
                }) {
                    error_toast(&gui, "Failed to disable Wi-Fi", e.into());
                }
                if let Err(e) = boot_normal(
                    &gui,
                    &boot_sender,
                    &set_page_sender,
                    &gui.get_default_user().to_string(),
                    first_boot_done,
                    login_credentials_sender.clone(),
                    core_settings_sender.clone(),
                ) {
                    error_toast(&gui, "Failed to send boot command", e.into())
                }
            }
        }
    });

    // Soft reset
    gui.on_soft_reset({
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                // Can be blocking because these operations should be relatively fast
                if let Err(e) = soft_reset() {
                    error_toast(&gui, "Failed to soft-reset", e.into());
                }
            }
        }
    });

    // Wi-Fi (toggle)
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

    // Wi-Fi (connect)
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

    // Wi-Fi (get networks)
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

    // Virtual keyboard
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

    // Brightness
    gui.on_set_brightness_sliders_levels({
        let gui_weak = gui_weak.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                // Assuming this will not fail. Otherwise, there would probably be something really wrong with the device...
                gui.set_cool_brightness(
                    brightness::get_brightness(&brightness::Mode::Cool).unwrap() * 100
                        / brightness::MAX_BRIGHTNESS,
                );
                gui.set_warm_brightness(
                    brightness::get_brightness(&brightness::Mode::Warm).unwrap() * 100
                        / brightness::MAX_BRIGHTNESS,
                );
            }
        }
    });

    gui.on_change_cool_brightness({
        move |value| {
            let _ = brightness::set_brightness_(
                value * brightness::MAX_BRIGHTNESS / 100,
                &brightness::Mode::Cool,
            );
        }
    });

    gui.on_change_warm_brightness({
        move |value| {
            let _ = brightness::set_brightness_(
                value * brightness::MAX_BRIGHTNESS / 100,
                &brightness::Mode::Warm,
            );
        }
    });

    // Battery status timer
    let battery_status_timer = Timer::default();
    battery_status_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        {
            let gui_weak = gui_weak.clone();
            let mut current_level: i32 = -1;
            let mut current_plug_status = false;
            move || {
                if let Ok(new_level) = battery::get_level() {
                    if let Some(gui) = gui_weak.upgrade() {
                        gui.set_battery_level(new_level);
                        if let Ok(charger_plugged_in) = battery::charger_plugged_in() {
                            let new_plug_status = charger_plugged_in;
                            if charger_plugged_in {
                                gui.set_charger_plugged_in(new_plug_status);
                                if new_plug_status != current_plug_status {
                                    if let Ok(icon) = Image::load_from_svg_data(include_bytes!(
                                        "../../icons/battery-charging.svg"
                                    )) {
                                        info!("Setting 'Charging' battery icon");
                                        gui.set_battery_icon(icon);
                                    }
                                }
                            } else {
                                gui.set_charger_plugged_in(new_plug_status);
                                if current_level != new_level
                                    || new_plug_status != current_plug_status
                                {
                                    if let Ok(icon) = Image::load_from_svg_data(
                                        battery::generate_svg_from_level(new_level).as_bytes(),
                                    ) {
                                        info!(
                                            "Changing battery icon for charge level {}",
                                            new_level
                                        );
                                        gui.set_battery_icon(icon);
                                    }
                                }
                            }
                            current_level = new_level;
                            current_plug_status = new_plug_status;
                        } else {
                            error!("Could not get battery status");
                        }
                    }
                } else {
                    error!("Could not get battery level");
                }
            }
        },
    );

    gui.on_login({
        let gui_weak = gui_weak.clone();
        let set_page_sender = set_page_sender.clone();
        let login_credentials_sender = login_credentials_sender.clone();
        move |username, password| {
            if let Some(gui) = gui_weak.upgrade() {
                if let Err(e) = storage_encryption::mount_storage(&username, &password) {
                    error_toast(&gui, "Login failed: please try again", e.into());
                } else {
                    if let Err(e) = login_credentials_sender.send(LoginForm {
                        username: username.to_string(),
                        password: password.to_string(),
                    }) {
                        error_toast(&gui, "Failed to send login credentials", e.into());
                    } else {
                        let _ = set_page_sender.send(Page::BootSplash);
                    }
                }
            }
        }
    });

    gui.on_change_initial_screen_rotation({
        let boot_config_mutex = boot_config_mutex.clone();
        move |index| {
            let mut locked_boot_config = boot_config_mutex.lock().unwrap();
            match index {
                0 => locked_boot_config.system.initial_screen_rotation = ScreenRotation::Cw0,
                1 => locked_boot_config.system.initial_screen_rotation = ScreenRotation::Cw90,
                2 => locked_boot_config.system.initial_screen_rotation = ScreenRotation::Cw180,
                3 | _ => locked_boot_config.system.initial_screen_rotation = ScreenRotation::Cw270,
            }
        }
    });

    gui.on_generate_splash_wallpaper({
        let gui_weak = gui_weak.clone();
        let splash_ready_sender = splash_ready_sender.clone();
        let boot_config_mutex = boot_config_mutex.clone();
        let can_shut_down = can_shut_down.clone();
        move |from_socket| {
            if let Some(gui) = gui_weak.upgrade() {
                let shut_down_command = gui.get_shutdown_command();
                if shut_down_command != RootFsShutDownCommand::Reboot {
                    if let Err(e) = splash::generate_wallpaper(&boot_config_mutex) {
                        error_toast(&gui, "Failed to generate wallpaper", e.into());
                    } else {
                        match Image::load_from_path(Path::new(splash::WALLPAPER_OUT_FILE_PATH)) {
                            Ok(wallpaper) => {
                                gui.set_splash_wallpaper(wallpaper);
                                let _ = fs::remove_file(&splash::WALLPAPER_OUT_FILE_PATH);
                            }
                            Err(e) => error_toast(&gui, "Failed to load wallpaper", e.into()),
                        }
                    }
                }

                match gui.get_shutdown_command() {
                    RootFsShutDownCommand::PowerOff => {
                        set_wallpaper_splash_text(&gui, &PrimitiveShutDownType::PowerOff)
                    }
                    RootFsShutDownCommand::Reboot => {
                        set_wallpaper_splash_text(&gui, &PrimitiveShutDownType::Reboot)
                    }
                    _ => {}
                };

                handle_screen_refresh(true, can_shut_down.clone());

                if from_socket {
                    let _ = splash_ready_sender.send(());
                }
            }
        }
    });

    gui.on_change_splash_wallpaper_model({
        let boot_config_mutex = boot_config_mutex.clone();
        move |wallpaper| {
            info!("Changing splash wallpaper model to '{}'", &wallpaper);
            let mut locked_boot_config = boot_config_mutex.lock().unwrap();
            locked_boot_config
                .system
                .splash_wallpaper_options
                .splash_wallpaper = Some(wallpaper.to_string());
        }
    });

    gui.on_refresh_screen({
        let can_shut_down = can_shut_down.clone();
        move |prepare_shut_down| {
            handle_screen_refresh(prepare_shut_down, can_shut_down.clone());
        }
    });

    let core_settings_receiver_timer = Timer::default();
    core_settings_receiver_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        {
            let gui_weak = gui_weak.clone();
            let finished = core_settings_finished_running.clone();
            let set_page_sender = set_page_sender.clone();
            let toast_sender = toast_sender.clone();
            let mut has_to_launch = false;
            move || {
                if has_to_launch {
                    if let Some(gui) = gui_weak.upgrade() {
                        if gui.get_startup_finished() {
                            has_to_launch = false;
                            thread_launch_core_settings(
                                &set_page_sender,
                                finished.clone(),
                                &toast_sender,
                            );
                        }
                    }
                }

                if let Ok(()) = core_settings_receiver.try_recv() {
                    info!("Received request to run Core Settings binary");
                    if let Some(gui) = gui_weak.upgrade() {
                        if gui.get_startup_finished() {
                            thread_launch_core_settings(
                                &set_page_sender,
                                finished.clone(),
                                &toast_sender,
                            );
                        } else {
                            has_to_launch = true;
                        }
                    }
                }
            }
        },
    );

    let core_settings_finished_running_timer = Timer::default();
    core_settings_finished_running_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(250),
        {
            let finished = core_settings_finished_running.clone();
            let set_page_sender = set_page_sender.clone();
            let boot_config = boot_config_mutex.clone();
            let gui_weak = gui_weak.clone();
            move || {
                if finished.load(Ordering::SeqCst) {
                    if let Some(gui) = gui_weak.upgrade() {
                        finished.store(false, Ordering::SeqCst);

                        if let Ok((new_boot_config, _)) = BootConfig::read() {
                            boot_config.lock().unwrap().system.default_user =
                                new_boot_config.system.default_user;
                        }

                        set_default_user_from_boot_config(&gui, boot_config.clone());
                        let _ = set_page_sender.send(Page::UserLogin);
                        gui.set_enable_ui(true);
                        if let Ok(buffer) =
                            Image::load_from_svg_data(include_bytes!("../../icons/settings.svg"))
                        {
                            gui.set_core_settings_button_icon(buffer);
                        }
                    }
                }
            }
        },
    );

    gui.on_launch_core_settings({
        let gui_weak = gui_weak.clone();
        let core_settings_sender = core_settings_sender.clone();
        move || {
            if let Some(gui) = gui_weak.upgrade() {
                gui.set_enable_ui(false);
                if !gui.get_startup_finished() {
                    if let Ok(buffer) =
                        Image::load_from_svg_data(include_bytes!("../../icons/hourglass-top.svg"))
                    {
                        gui.set_core_settings_button_icon(buffer);
                    }
                }
                let _ = core_settings_sender.send(());
            }
        }
    });

    gui.run()?;

    Ok(())
}

fn toast(gui: &AppWindow, message: &str) {
    gui.set_sticky_toast(false);
    gui.set_dialog_message(SharedString::from(message));
    gui.set_dialog(DialogType::Toast);
    info!("{}", &message);
}

fn error_toast(gui: &AppWindow, message: &str, e: anyhow::Error) {
    gui.set_sticky_toast(false);
    gui.set_dialog_message(SharedString::from(message));
    gui.set_dialog(DialogType::Toast);
    error!("{}: {}", &message, e);
}

fn boot_normal(
    gui: &AppWindow,
    boot_sender: &Sender<BootCommandForm>,
    set_page_sender: &Sender<Page>,
    default_user: &str,
    first_boot_done: bool,
    login_credentials_sender: Sender<LoginForm>,
    core_settings_sender: Sender<()>,
) -> Result<()> {
    let mut wait_for_login = false;
    let default_user = default_user.to_string();

    if first_boot_done {
        let encryption_users_list = storage_encryption::get_users_using_storage_encryption()?;
        if !encryption_users_list.is_empty()
            && encryption_users_list.contains(&default_user)
            && storage_encryption::get_user_storage_encryption_status(&default_user)?
        {
            wait_for_login = true;
        } else {
            if default_user.is_empty() {
                wait_for_login = true;
            } else {
                info!(
                    "Triggering automatic login for default user '{}'",
                    &default_user
                );
                let _ = boot_sender.send(BootCommandForm {
                    command: BootCommand::NormalBoot,
                    can_shut_down: None,
                });
                storage_encryption::mount_storage(
                    &default_user,
                    &storage_encryption::DISABLED_MODE_PASSWORD,
                )?;
                if let Err(e) = login_credentials_sender.send(LoginForm {
                    username: default_user,
                    password: storage_encryption::DISABLED_MODE_PASSWORD.to_string(),
                }) {
                    error_toast(
                        &gui,
                        "Failed to send credentials for automatic login",
                        e.into(),
                    );
                }
            }
        }

        if wait_for_login {
            set_page_sender.send(Page::UserLogin)?;
            let _ = boot_sender.send(BootCommandForm {
                command: BootCommand::NormalBoot,
                can_shut_down: None,
            });
        }
    } else {
        info!("First boot has not been done yet: triggering OOBE");
        let _ = boot_sender.send(BootCommandForm {
            command: BootCommand::NormalBoot,
            can_shut_down: None,
        });
        let _ = core_settings_sender.send(());
    }

    Ok(())
}

fn determine_power_down_mode(gui: &AppWindow) -> PowerDownMode {
    let power_down_mode: PowerDownMode;
    if gui.get_shutdown_command() == RootFsShutDownCommand::None {
        power_down_mode = PowerDownMode::Normal;
    } else {
        power_down_mode = PowerDownMode::RootFS;
    }

    return power_down_mode;
}

fn gui_shut_down(
    gui: &AppWindow,
    shut_down_type: PrimitiveShutDownType,
    mode: PowerDownMode,
    can_shut_down: Arc<AtomicBool>,
) -> Result<()> {
    set_wallpaper_splash_text(&gui, &shut_down_type);
    thread::spawn(move || shut_down(shut_down_type, mode, can_shut_down.clone()));

    Ok(())
}

fn set_wallpaper_splash_text(gui: &AppWindow, shut_down_type: &PrimitiveShutDownType) {
    match shut_down_type {
        PrimitiveShutDownType::PowerOff => {
            let current_time: DateTime<Local> = Local::now();
            gui.set_splash_wallpaper_text(SharedString::from("Powered off"));
            gui.set_splash_wallpaper_date_time_information(SharedString::from(
                current_time.format("%d/%m").to_string(),
            ));
        }
        PrimitiveShutDownType::Reboot => {
            gui.set_splash_wallpaper_text(SharedString::from("Rebooting"));
            gui.set_splash_wallpaper_date_time_information(gui.get_current_time());
        }
        PrimitiveShutDownType::Sleep => {
            gui.set_splash_wallpaper_text(SharedString::from("Sleeping"));
            gui.set_splash_wallpaper_date_time_information(gui.get_current_time());
        }
    }
}

fn handle_screen_refresh(prepare_shut_down: bool, can_shut_down: Arc<AtomicBool>) {
    if prepare_shut_down {
        let can_shut_down = can_shut_down.clone();
        thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(2000));
            eink::full_refresh();
            can_shut_down.store(true, Ordering::SeqCst);
        });
    } else {
        eink::full_refresh();
    }
}

fn thread_launch_core_settings(
    set_page_sender: &Sender<Page>,
    finished: Arc<AtomicBool>,
    toast_sender: &Sender<String>,
) {
    let _ = set_page_sender.send(Page::None);
    thread::spawn({
        let finished = finished.clone();
        let toast_sender = toast_sender.clone();
        move || {
            if let Err(e) = system::run_core_settings() {
                let err_msg = "Failed to run Core Settings binary".to_string();
                error!("{}: {}", &err_msg, &e);
                let _ = toast_sender.send(err_msg);
            }
            finished.store(true, Ordering::SeqCst);
        }
    });
}

fn set_default_user_from_boot_config(gui: &AppWindow, boot_config: Arc<Mutex<BootConfig>>) {
    if let Some(user) = &boot_config.lock().unwrap().system.default_user {
        info!("Found default user in boot configuration: '{}'", &user);
        gui.set_default_user(SharedString::from(format!("{}", &user)));
    } else {
        info!("Did not find a default user in boot configuration");
    }
}
