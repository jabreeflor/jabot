// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--aider-acp") {
        std::process::exit(jabot_lib::run_aider_acp());
    }
    jabot_lib::run()
}
