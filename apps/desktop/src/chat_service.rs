//! Public chat commands and a cancellable background subscription.
use kova_client_core::{Cache, Instance, Result, chat::ChatScope, vault::Vault};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub enum Command {
    Sync,
    Send(String),
    Retry,
    Offline,
}
fn scope(vault: &Vault, origin: &str, channel: &str) -> Result<ChatScope> {
    ChatScope::new(origin, &vault.identity()?.global_id()?, channel)
}
fn display(cache: &Cache, scope: &ChatScope, status: &str) -> Result<String> {
    let mut text = format!("{status}\n{} · {}\n", scope.origin, scope.channel);
    for message in cache.cached_messages(scope)? {
        text.push_str(&format!(
            "\n#{} · {} · {}\n{}\n",
            message.seq, message.author_id, message.created_at, message.body
        ));
    }
    if let Some(pending) = cache.pending_message(scope)? {
        text.push_str(&format!(
            "\nEnvoi en attente — utilisez Réessayer :\n{}",
            pending.body
        ));
    }
    Ok(text)
}
pub fn execute(vault: &Vault, origin: &str, channel: &str, command: Command) -> Result<String> {
    let cache = Cache::open(&vault.directory()?.join("cache.sqlite"))?;
    let scope = scope(vault, origin, channel)?;
    if matches!(command, Command::Offline) {
        return display(
            &cache,
            &scope,
            "Hors ligne — copie locale, accès actuel non vérifié.",
        );
    }
    let instance = Instance::new(origin)?;
    let session = vault.session(origin)?;
    let sending = matches!(command, Command::Send(_) | Command::Retry);
    match command {
        Command::Send(body) => {
            let pending = cache.prepare_message(&scope, &body)?;
            let ack = instance.send_message(&session, channel, &pending)?;
            cache.acknowledge_message(&scope, &ack)?;
        }
        Command::Retry => {
            let pending = cache
                .pending_message(&scope)?
                .ok_or("Aucun envoi en attente")?;
            let ack = instance.send_message(&session, channel, &pending)?;
            cache.acknowledge_message(&scope, &ack)?;
        }
        _ => {}
    }
    if let Err(error) = instance.sync_messages(&session, &cache, channel) {
        if sending {
            return Ok(format!(
                "Message confirmé par le serveur. Actualisation impossible : {error}. Synchronisez pour actualiser l’historique."
            ));
        }
        return Err(error);
    }
    display(
        &cache,
        &scope,
        "Historique synchronisé (100 derniers messages affichés).",
    )
}

pub struct Subscription {
    stop: Arc<AtomicBool>,
    pub latest: Arc<Mutex<Option<String>>>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl Subscription {
    pub fn start(profile: String, origin: String, channel: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let latest = Arc::new(Mutex::new(None));
        let worker_stop = stop.clone();
        let worker_latest = latest.clone();
        std::thread::spawn(move || {
            let publish = |text: String| {
                if let Ok(mut slot) = worker_latest.lock() {
                    *slot = Some(text);
                }
            };
            if let Err(error) = follow(&profile, &origin, &channel, &worker_stop, &publish) {
                publish(format!("Flux arrêté : {error}"));
            }
        });
        Self { stop, latest }
    }
}
fn follow(
    profile: &str,
    origin: &str,
    channel: &str,
    stop: &AtomicBool,
    publish: &impl Fn(String),
) -> Result<()> {
    let vault = Vault::new(profile)?;
    let cache = Cache::open(&vault.directory()?.join("cache.sqlite"))?;
    let scope = scope(&vault, origin, channel)?;
    let instance = Instance::new(origin)?;
    let mut delay = 1;
    while !stop.load(Ordering::Relaxed) {
        let session = vault.session(origin)?;
        // Local expiry is terminal; reconnecting requires a new signed login.
        instance.chat_scope(&session, channel)?;
        let attempt = (|| -> Result<()> {
            let mut stream =
                instance.subscribe_chat(&session, channel, cache.chat_cursor(&scope)?)?;
            let mut first = true;
            while !stop.load(Ordering::Relaxed) {
                if let Some(page) = stream.read_page()? {
                    cache.merge_chat_page(&scope, &page)?;
                    if first || !page.items.is_empty() {
                        publish(display(&cache, &scope, "Connecté — réception en direct.")?);
                    }
                    first = false;
                }
            }
            Ok(())
        })();
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Err(error) = attempt {
            // A fresh HTTP authorization distinguishes a lost transport from lost access.
            // No cached text is shown as live while the server is unreachable.
            if let Err(check) = instance.messages(&session, channel, cache.chat_cursor(&scope)?) {
                let text = check.to_string();
                if text.starts_with("401") || text.starts_with("403") || text.starts_with("404") {
                    return Err(check);
                }
            }
            publish(format!(
                "Déconnecté : {error}\nReprise automatique dans {delay} s. La copie locale reste disponible via Hors ligne."
            ));
        }
        for _ in 0..delay * 10 {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        delay = (delay * 2).min(8);
    }
    Ok(())
}
