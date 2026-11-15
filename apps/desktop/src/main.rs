#[cfg(windows)]
mod chat_service;
#[cfg(windows)]
mod community_service;
#[cfg(windows)]
mod dm_service;
#[cfg(windows)]
mod identity_service;
#[cfg(windows)]
mod identity_ui;
#[cfg(windows)]
mod input;
#[cfg(windows)]
mod spike;
#[cfg(windows)]
mod workspace_service;
#[cfg(windows)]
mod workspace_ui;
#[cfg(windows)]
fn main() {
    if std::env::args().any(|arg| arg == "--spike") {
        spike::run();
    } else if std::env::args().any(|arg| arg == "--diagnostics") {
        identity_ui::run();
    } else {
        workspace_ui::run();
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This prototype targets Windows only; other desktops are not validated.");
}
