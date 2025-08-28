use anyhow::{Context, Result};
use std::fs;

const CHARGER_ONLINE_PATH: &str = "/sys/class/power_supply/rk817-charger/online";
const LEVEL_PATH: &str = "/sys/class/power_supply/rk817-battery/capacity";

const MAX_BAR_WIDTH: i32 = 540;
const BATTERY_BASE_B: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" height="24px" viewBox="0 -960 960 960" width="24px" fill="#000000"><path d="M160-240q-50 0-85-35t-35-85v-240q0-50 35-85t85-35h540q50 0 85 35t35 85v240q0 50-35 85t-85 35H160Zm0-80h540q17 0 28.5-11.5T740-360v-240q0-17-11.5-28.5T700-640H160q-17 0-28.5 11.5T120-600v240q0 17 11.5 28.5T160-320Zm700-60v-200h20q17 0 28.5 11.5T920-540v120q0 17-11.5 28.5T880-380h-20Zm-700 20v-240h"##;
const BATTERY_BASE_E: &str = r##"v240h-80Z"/></svg>"##;

pub enum BatteryState {
    Charging,
    NotCharging,
    Critical,
}

pub fn generate_svg_from_level(level: i32) -> String {
    return format!(
        "{}{}{}",
        &BATTERY_BASE_B,
        level * MAX_BAR_WIDTH / 100,
        &BATTERY_BASE_E
    );
}

pub fn get_level() -> Result<i32> {
    Ok(fs::read_to_string(&LEVEL_PATH)?
        .trim()
        .parse::<i32>()
        .with_context(|| "Failed to read battery level")?)
}

pub fn charger_plugged_in() -> Result<bool> {
    Ok(fs::read_to_string(&CHARGER_ONLINE_PATH)
        .with_context(|| "Failed to read charger status")?
        .contains("1"))
}
