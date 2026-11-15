mod auth;
mod chat;
mod communities;
mod config;
mod dm;
mod replication;

use std::{env, error::Error, time::Duration};

use axum::{Router, extract::State, http::StatusCode, routing::get};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::{net::TcpListener, signal, time::timeout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if let [command, path] = arguments.as_slice()
        && command == "generate-federation-key"
    {
        return generate_federation_key(std::path::Path::new(path));
    }
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect_with(config::postgres_connect_options()?)
        .await?;
    sqlx::migrate!().run(&pool).await?;

    match arguments.as_slice() {
        [] => (),
        [cmd, handle, name, key] if cmd == "bootstrap-admin" || cmd == "bind-profile" => {
            return auth::bootstrap(&pool, handle, name, key, cmd == "bootstrap-admin").await;
        }
        _ => {
            return Err(
                "Usage : kova-server [bootstrap-admin|bind-profile HANDLE NOM CLE_PUBLIQUE_HEX] | generate-federation-key PATH"
                    .into(),
            );
        }
    }
    let origin = env::var("KOVA_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let address = env::var("KOVA_LISTEN").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = TcpListener::bind(address).await?;
    eprintln!("Kova écoute sur {}", listener.local_addr()?);
    let app = Router::new()
        .route("/health/ready", get(ready))
        .with_state(pool.clone())
        .nest("/api/v1", auth::routes(pool, &origin)?);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    Ok(())
}

fn generate_federation_key(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::io::Write;

    let document = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .map_err(|_| "Impossible de générer la clé de fédération")?;
    let pair = Ed25519KeyPair::from_pkcs8(document.as_ref())
        .map_err(|_| "Clé de fédération générée invalide")?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(document.as_ref())?;
    file.sync_all()?;
    println!("{}", hex::encode(pair.public_key().as_ref()));
    Ok(())
}

async fn ready(State(pool): State<PgPool>) -> StatusCode {
    match timeout(
        Duration::from_secs(2),
        sqlx::query("SELECT 1").execute(&pool),
    )
    .await
    {
        Ok(Ok(_)) => StatusCode::OK,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("installation du gestionnaire SIGTERM");
        tokio::select! {
            _ = signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = signal::ctrl_c().await;
}
