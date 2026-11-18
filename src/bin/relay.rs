use anyhow::{Context, Result};
use desklink::relay::server::{serve, RelayConfig};

#[tokio::main]
async fn main() -> Result<()> {
    let address = std::env::var("DESKLINK_RELAY_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let config = RelayConfig {
        access_code: std::env::var("DESKLINK_RELAY_ACCESS_CODE").ok(),
        allow_public_enrollment: std::env::var("DESKLINK_RELAY_PUBLIC_ENROLLMENT")
            .is_ok_and(|v| v == "1"),
        proxy_token: std::env::var("DESKLINK_RELAY_PROXY_TOKEN").ok(),
        directory_path: Some(
            std::env::var_os("DESKLINK_RELAY_DIRECTORY")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| "desklink-directory.sqlite3".into()),
        ),
        allow_unauthenticated_loopback: std::env::var("DESKLINK_RELAY_DEV_LOOPBACK")
            .is_ok_and(|v| v == "1"),
        ..RelayConfig::default()
    };
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .context("Could not bind relay")?;
    eprintln!(
        "DeskLink relay listening on {} (TLS must be supplied by the reverse proxy)",
        listener.local_addr()?
    );
    serve(listener, config, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}
