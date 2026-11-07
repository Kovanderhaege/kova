#[cfg(windows)]
mod identity_service;
#[cfg(windows)]
mod identity_ui;
#[cfg(windows)]
mod input;
#[cfg(windows)]
mod spike;
#[cfg(windows)]
fn main() {
    if std::env::args().any(|arg| arg == "--spike") {
        spike::run();
    } else {
        identity_ui::run();
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This prototype targets Windows only; other desktops are not validated.");
}
