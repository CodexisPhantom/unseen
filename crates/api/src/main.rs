use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `api openapi <file>` writes the OpenAPI document and exits (`just gen`).
    let args: Vec<String> = std::env::args().collect();
    if let [_, command, file] = args.as_slice()
        && command == "openapi"
    {
        std::fs::write(file, api::openapi().to_pretty_json()? + "\n")?;
        return Ok(());
    }

    // Not `fmt::init()`: once a dependency enables tracing-subscriber's
    // `env-filter` feature, it shows only errors unless RUST_LOG is set.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "unseen starting");

    let data_dir = PathBuf::from(std::env::var("UNSEEN_DATA_DIR").unwrap_or("/config".into()));
    std::fs::create_dir_all(&data_dir)?;
    let db = db::connect(&data_dir).await?;
    tracing::info!(data_dir = %data_dir.display(), "database ready");

    // Files adopted before the parser existed have quality `Unknown`.
    let backfill = db::backfill::run(&db).await?;
    if backfill.updated > 0 {
        tracing::info!(files = backfill.updated, "quality set on adopted files");
    }
    for path in &backfill.unparsed {
        tracing::warn!(%path, "quality of adopted file not parsed, stays Unknown");
    }

    let secret = api::auth::load_secret(std::env::var("UNSEEN_SECRET").ok(), &data_dir)?;
    let base_url = std::env::var("UNSEEN_BASE_URL").ok();
    let state = api::AppState::new(db, &secret, base_url.as_deref()).await?;

    let bind = std::env::var("UNSEEN_BIND").unwrap_or("0.0.0.0:7979".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "listening");
    axum::serve(
        listener,
        api::router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    tracing::info!("stopped");
    Ok(())
}

/// Resolves on Ctrl+C, and on SIGTERM (what `docker stop` sends). The process
/// is PID 1 in the container, where SIGTERM does nothing without a handler.
async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}
