mod api;
mod config;
mod db;
mod error;
mod metadata;
mod models;
mod readers;
mod scanner;

use std::time::Duration;

use api::AppState;
use axum::Router;
use config::Config;
use db::Database;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "estante_livre=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env();
    tokio::fs::create_dir_all(&config.covers_dir).await?;
    let db = Database::open(&config.database_path, &config.initial_library_root).await?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("EstanteLivre/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()?;
    let state = AppState {
        db,
        config: config.clone(),
        http,
    };
    let app = Router::new()
        .nest("/api", api::router(state))
        .fallback_service(ServeDir::new("web").append_index_html_on_directories(true))
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    tracing::info!(address = %config.bind, "Estante Livre pronta");
    axum::serve(listener, app).await?;
    Ok(())
}
