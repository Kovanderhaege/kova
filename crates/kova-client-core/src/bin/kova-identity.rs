//! Diagnostic client: never prints private keys or session tokens.
#[cfg(windows)]
fn main() -> kova_client_core::Result<()> {
    use kova_client_core::{Identity, Instance, vault};
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command] if command == "init" => {
            let identity = Identity::generate()?;
            vault::save_identity(&identity)?;
            println!("{}", identity.root_public_key()?);
        }
        [command, origin] if command == "login" => {
            let session = Instance::new(origin)?.login(&vault::identity()?, None)?;
            vault::save_session(&session)?;
            println!("{}", session.profile);
        }
        [command, origin] if command == "me" => {
            println!("{}", Instance::new(origin)?.me(&vault::session(origin)?)?)
        }
        [command, origin] if command == "logout" => {
            Instance::new(origin)?.logout(&vault::session(origin)?)?;
            vault::forget_session(origin)?;
        }
        _ => return Err("Usage: kova-identity init | login|me|logout ORIGIN".into()),
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("The OS vault in this diagnostic client currently requires Windows.");
    std::process::exit(1);
}
