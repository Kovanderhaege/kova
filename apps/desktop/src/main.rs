#[cfg(windows)]
mod spike;
#[cfg(windows)]
fn main() {
    spike::run();
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This prototype targets Windows only; other desktops are not validated.");
}
