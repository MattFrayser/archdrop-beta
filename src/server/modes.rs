use super::utils;
use crate::server::ServerDirection;
use crate::tunnel::CloudflareTunnel;
use crate::{output, qr};
use anyhow::{Context, Result};
use axum::Router;
use std::net::SocketAddr;
use tokio::sync::watch;

pub struct Server {
    pub app: Router,
    pub token: String,
    pub key: String,
    pub nonce: String,
    pub file_name: String,
    pub progress_consumer: watch::Receiver<f64>,
}

pub async fn start_local(server: Server, direction: ServerDirection) -> Result<u16> {
    let spinner = output::spinner("Starting local HTTPS server...");
    // local Ip and Certs
    let local_ip = utils::get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let tls_config = utils::generate_cert(&local_ip)
        .await
        .context("Failed to generate TLS certificate")?;

    // spawn server server
    let addr = SocketAddr::from(([127, 0, 0, 0], 0));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context(format!("Failed to bind to {}", addr))?;
    let port = listener.local_addr()?.port();

    spinner.set_message(format!("Waiting for server on port {}...", port));

    utils::wait_for_server_ready(port, 5)
        .await
        .context("Server failed to become ready")?;

    output::finish_spinner_success(&spinner, &format!("Server ready on port {}", port));

    let service = match direction {
        ServerDirection::Send => "download",
        ServerDirection::Receive => "upload",
    };

    let url = format!(
        "https://{}:{}/{}/{}#key={}&nonce={}",
        local_ip, port, service, server.token, server.key, server.nonce
    );
    println!("{}", url);

    let qr_code = qr::generate_qr(&url)?;
    let tui_handle = utils::spawn_tui(
        server.progress_consumer,
        server.file_name,
        qr_code,
        service == "upload",
    );

    // HTTPS Server
    let handle = axum_server::Handle::new();

    // Spawn shutdown waiter
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = tui_handle => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        shutdown_handle.shutdown();
    });

    // Start server
    axum_server::bind_rustls(addr, tls_config)
        .handle(handle)
        .serve(server.app.into_make_service())
        .await?;

    Ok(port)
}

pub async fn start_http(server: Server, direction: ServerDirection) -> Result<u16> {
    // Start local HTTP
    let spinner = output::spinner("Starting local server...");
    let (port, server_handle) = spawn_http_server(server.app)
        .await
        .context("Failed to spawn HTTP server")?;

    spinner.set_message(format!("Waiting for server on port {}...", port));

    // Wait for server to be ready before starting tunnel
    utils::wait_for_server_ready(port, 5)
        .await
        .context("Server failed to become ready")?;

    output::finish_spinner_success(&spinner, &format!("Server ready on port {}", port));

    let service = match direction {
        ServerDirection::Send => "download",
        ServerDirection::Receive => "upload",
    };

    let url = format!(
        "http://127.0.0.1:{}/{}/{}#key={}&nonce={}",
        port, service, server.token, server.key, server.nonce
    );
    println!("{url}");

    // Spawn TUI and get handle
    let qr_code = qr::generate_qr(&url)?;
    let tui_handle = utils::spawn_tui(
        server.progress_consumer,
        server.file_name,
        qr_code,
        service == "upload",
    );

    // Wait for TUI to exit or Ctrl+C
    tokio::select! {
        _ = tui_handle => {}
        _ = tokio::signal::ctrl_c() => {}
    }

    // Graceful shutdown
    server_handle.shutdown();

    Ok(port)
}

pub async fn start_tunnel(server: Server, direction: ServerDirection) -> Result<u16> {
    // Start local HTTP
    let spinner = output::spinner("Starting local server...");
    let (port, server_handle) = spawn_http_server(server.app)
        .await
        .context("Failed to spawn HTTP server")?;
    spinner.set_message(format!("Waiting for server on port {}...", port));

    // Wait for server to be ready before starting tunnel
    utils::wait_for_server_ready(port, 5)
        .await
        .context("Server failed to become ready")?;
    output::finish_spinner_success(&spinner, &format!("Server ready on port {}", port));

    // Start tunnel
    let tunnel = CloudflareTunnel::start(port)
        .await
        .context("Failed to establish Cloudflare tunnel")?;

    let service = match direction {
        ServerDirection::Send => "download",
        ServerDirection::Receive => "upload",
    };

    let url = format!(
        "{}/{}/{}#key={}&nonce={}",
        tunnel.url(),
        service,
        server.token,
        server.key,
        server.nonce
    );
    println!("{}", url);

    // Spawn TUI and get handle
    let qr_code = qr::generate_qr(&url)?;
    let tui_handle = utils::spawn_tui(
        server.progress_consumer,
        server.file_name,
        qr_code,
        service == "upload",
    );

    // Wait for TUI to exit or Ctrl+C
    tokio::select! {
        _ = tui_handle => {}
        _ = tokio::signal::ctrl_c() => {}
    }

    // Graceful shutdown
    server_handle.shutdown();

    Ok(port)
}

async fn spawn_http_server(
    app: Router,
) -> Result<(u16, axum_server::Handle)> {
    // Get random port
    let addr = SocketAddr::from(([0, 0, 0, 0], 0));

    // bind to socket
    let std_listener = std::net::TcpListener::bind(addr)?;
    let port = std_listener.local_addr()?.port();

    // Spawn server in background
    let handle = axum_server::Handle::new();
    let server_handle = handle.clone();

    tokio::spawn(async move {
        if let Err(e) = axum_server::from_tcp(std_listener)
            .handle(server_handle)
            .serve(app.into_make_service())
            .await
        {
            eprintln!("Server error: {}", e);
        }
    });

    Ok((port, handle))
}
