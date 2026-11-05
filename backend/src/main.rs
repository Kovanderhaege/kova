mod auth;

use std::{env, error::Error, time::Duration};

use axum::{Router, extract::State, http::StatusCode, routing::get};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::{net::TcpListener, signal, time::timeout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect_with(PgConnectOptions::new())
        .await?;
    sqlx::migrate!().run(&pool).await?;

    match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => (),
        [cmd] if cmd == "create-first-user" => return auth::bootstrap(&pool).await,
        _ => return Err("Usage : kova-server [create-first-user]".into()),
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
