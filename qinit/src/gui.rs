use anyhow::{Context, Result};
slint::include_modules!();

pub fn setup_gui() -> Result<()> {
    let ui = AppWindow::new()?;

    ui.run()?;

    Ok(())
}
