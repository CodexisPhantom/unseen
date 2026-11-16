fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "unseen starting");
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {}
}
