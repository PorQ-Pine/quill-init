use chrono::{Datelike, Utc};

fn main() {
    let year = Utc::now().year();
    println!("cargo:rustc-env=BUILD_YEAR={}", year);

    slint_build::compile("ui/app-window.slint").expect("Slint build failed");
}
