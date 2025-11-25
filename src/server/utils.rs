use crate::tui::TransferUI;
use std::net::UdpSocket;
use tokio::signal;
use tokio::sync::watch;

use axum_server::tls_rustls::RustlsConfig;
use rcgen::generate_simple_self_signed;

pub async fn wait_for_server_ready(
    port: u16,
    timeout_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()?;

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    loop {
        if start.elapsed() > timeout {
            return Err("Server failed to start within timeout".into());
        }

        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                return Ok(());
            }
            _ => {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    }
}
pub fn spawn_tui(
    progress: watch::Receiver<f64>,
    file_name: String,
    file_hash: String,
    qr_code: String,
    url: String,
) {
    tokio::spawn(async move {
        let mut ui = TransferUI::new(progress, file_name, file_hash, qr_code, url);

        if let Err(e) = ui.run().await {
            eprintln!("ui err: {}", e);
        }
    });
}

pub fn shutdown_handler(handle: axum_server::Handle) {
    // Spawn ctrl-c handler
    tokio::spawn(async move {
        signal::ctrl_c().await.ok();
        handle.shutdown();
    });
}

pub fn get_local_ip() -> Option<String> {
    // Connect to external address to determine local ip
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip().to_string())
}

pub async fn generate_cert(ip: &str) -> Result<RustlsConfig, Box<dyn std::error::Error>> {
    let subject_alt_names = vec![ip.to_string(), "localhost".to_string()];

    let cert = generate_simple_self_signed(subject_alt_names)?;
    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();

    tokio::fs::write("/tmp/archdrop-cert.pem", &cert_pem).await?;
    tokio::fs::write("/tmp/archdrop-key.pem", &key_pem).await?;

    Ok(RustlsConfig::from_pem_file("/tmp/archdrop-cert.pem", "/tmp/archdrop-key.pem").await?)
}
