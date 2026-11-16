use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Not `fmt::init()`: once a dependency enables tracing-subscriber's
    // `env-filter` feature, it shows only errors unless RUST_LOG is set.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "unseen starting");

    let data_dir = PathBuf::from(std::env::var("UNSEEN_DATA_DIR").unwrap_or("/config".into()));
    std::fs::create_dir_all(&data_dir)?;
    let _db = db::connect(&data_dir).await?;
    tracing::info!(data_dir = %data_dir.display(), "database ready");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {}
}
