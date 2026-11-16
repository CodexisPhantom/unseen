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

    let bind = std::env::var("UNSEEN_BIND").unwrap_or("0.0.0.0:7979".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "listening");
    axum::serve(listener, api::router(api::AppState { db })).await?;
    Ok(())
}
