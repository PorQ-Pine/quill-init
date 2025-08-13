use anyhow::{Context, Result};
use std::fs;
use std::time::Duration;
use std::thread;
use log::debug;

const BACKLIGHT_COOL_NODE_W: &str = "/sys/class/backlight/backlight_cool/brightness";
const BACKLIGHT_WARM_NODE_W: &str = "/sys/class/backlight/backlight_warm/brightness";
const BACKLIGHT_COOL_NODE_R: &str = "/sys/class/backlight/backlight_cool/actual_brightness";
const BACKLIGHT_WARM_NODE_R: &str = "/sys/class/backlight/backlight_warm/actual_brightness";
const DELAY: Duration = Duration::from_millis(1);

pub const MAX_BRIGHTNESS: i32 = 255;

#[derive(Debug)]
pub enum Mode {
    Cool,
    Warm,
}

pub fn set_brightness_(level: i32, mode: &Mode) -> Result<()> {
    let node = match mode {
        Mode::Cool => BACKLIGHT_COOL_NODE_W,
        Mode::Warm => BACKLIGHT_WARM_NODE_W,
    };
    fs::write(&node, level.to_string()).with_context(|| format!("Failed to write to {:?} brightness sysfs node", &node))?;

    Ok(())
}

pub fn get_brightness(mode: &Mode) -> Result<i32> {
    let node = match mode {
        Mode::Cool => BACKLIGHT_COOL_NODE_R,
        Mode::Warm => BACKLIGHT_WARM_NODE_R,
    };

    let value: i32 = fs::read_to_string(&node).with_context(|| format!("Failed to read {:?} brightness sysfs node", &node))?.trim().parse().with_context(|| "Failed to parse brightness from {:?} brightness sysfs node")?;

    Ok(value)
}

pub fn set_brightness(level_to_set: i32, mode: &Mode) -> Result<()> {
    let mut current_level = get_brightness(&mode)?;
    while current_level != level_to_set {
        if current_level < level_to_set {
            current_level += 1;
        } else {
            current_level -= 1;
        }
        debug!("Setting {:?} brightness to level {}", &mode, &current_level);
        set_brightness_(current_level, &mode)?;
        thread::sleep(DELAY);
    }

    Ok(())
}

pub fn set_brightness_unified(level_cool: i32, level_warm: i32) -> Result<()> {
    let thread_cool = thread::spawn(move || -> Result<()> {
        set_brightness(level_cool, &Mode::Cool)?;
        return Ok(())
    });
    let thread_warm = thread::spawn(move || -> Result<()> {
        set_brightness(level_warm, &Mode::Warm)?;
        return Ok(())
    });

    thread_cool.join().map_err(|e| anyhow::anyhow!("Thread for setting cool brightness panicked: {:?}", e))??;
    thread_warm.join().map_err(|e| anyhow::anyhow!("Thread for setting warm brightness panicked: {:?}", e))??;

    Ok(())
}
