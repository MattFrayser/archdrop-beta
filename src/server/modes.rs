use super::utils;
use crate::server::state::{AppState, ServerInstance};
use crate::server::ServerDirection;
use crate::tunnel::CloudflareTunnel;
use crate::types::Nonce;
use crate::ui::{output, qr};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::time::Duration;

enum Protocol {
    Https,
    Http,
}
pub async fn start_https(
    server: ServerInstance,
    app_state: AppState,
    direction: ServerDirection,
    nonce: Nonce,
) -> Result<u16> {
    let service = direction_to_str(direction);

    // Clone needed before consuming server
    let session = server.session.clone();
    let display_name = server.display_name.clone();
    let progress_receiver = server.progress_receiver();

    let (port, server_handle) = start_local_server(server, Protocol::Https).await?;

    let base_url = format!("https://127.0.0.1:{}", port);
    let url = format!(
        "{}/{}/{}#key={}&nonce={}",
        base_url,
        service,
        session.token(),
        session.session_key_b64(),
        nonce.to_base64()
    );

    println!("{}", url);

    run_session(server_handle, app_state, display_name, progress_receiver, url, service).await?;
    Ok(port)
}

pub async fn start_tunnel(
    server: ServerInstance,
    app_state: AppState,
    direction: ServerDirection,
    nonce: Nonce,
) -> Result<u16> {
    let service = direction_to_str(direction);

    // Clone what we need before consuming server
    let session = server.session.clone();
    let display_name = server.display_name.clone();
    let progress_receiver = server.progress_receiver();

    let (port, server_handle) = start_local_server(server, Protocol::Http).await?;

    // Start tunnel
    let tunnel = CloudflareTunnel::start(port)
        .await
        .context("Failed to establish Cloudflare tunnel")?;

    // Ensure tunnel URL doesn't have trailing slash
    let tunnel_url = tunnel.url().trim_end_matches('/');
    let url = format!(
        "{}/{}/{}#key={}&nonce={}",
        tunnel_url,
        service,
        session.token(),
        session.session_key_b64(),
        nonce.to_base64()
    );
    println!("{}", url);

    run_session(server_handle, app_state, display_name, progress_receiver, url, service).await?;

    // Drop tunnel explicitly to ensure cleanup
    // Give a moment for cleanup
    drop(tunnel);
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    Ok(port)
}

async fn start_local_server(
    server: ServerInstance,
    protocol: Protocol,
) -> Result<(u16, axum_server::Handle)> {
    let spinner = output::spinner("Starting local HTTPS server...");
    // Bind to random port
    let addr = SocketAddr::from(([0, 0, 0, 0], 0));
    let listener = std::net::TcpListener::bind(addr).context("Failed to bind socket")?;

    listener
        .set_nonblocking(true)
        .context("Failed to set listener to non-blocking mode")?;

    let port = listener.local_addr()?.port();

    // Spawn HTTP server in background
    let server_handle = axum_server::Handle::new();
    let server_handle_clone = server_handle.clone();

    // HTTPS uses self signed certs
    match protocol {
        Protocol::Https => {
            let tls_config = utils::generate_cert("127.0.0.1")
                .await
                .context("Failed to generate TLS certificate")?;
            tokio::spawn(async move {
                if let Err(e) = axum_server::from_tcp_rustls(listener, tls_config)
                    .handle(server_handle_clone)
                    .serve(server.app.into_make_service())
                    .await
                {
                    eprintln!("Server error: {}", e);
                }
            });
        }
        Protocol::Http => {
            tokio::spawn(async move {
                if let Err(e) = axum_server::from_tcp(listener)
                    .handle(server_handle_clone)
                    .serve(server.app.into_make_service())
                    .await
                {
                    eprintln!("Server error: {}", e);
                }
            });
        }
    }

    let use_https = matches!(protocol, Protocol::Https);
    utils::wait_for_server_ready(port, 5, use_https)
        .await
        .context("Server failed to become ready")?;
    output::spinner_success(&spinner, &format!("Server ready on port {}", port));

    Ok((port, server_handle))
}

async fn run_session(
    server_handle: axum_server::Handle,
    app_state: AppState,
    display_name: String,
    progress_receiver: tokio::sync::watch::Receiver<f64>,
    url: String,
    service: &str,
) -> Result<()> {
    // Spawn TUI and get handle
    let qr_code = qr::generate_qr(&url)?;
    let tui_handle = utils::spawn_tui(
        progress_receiver,
        display_name,
        qr_code,
        service == "upload",
    );

    // Wait for TUI to exit or Ctrl+C
    tokio::select! {
        _ = tui_handle => {}
        _ = tokio::signal::ctrl_c() => {}
    }

    // Graceful shutdown with 10 second grace period
    const SHUTDOWN_TIMEOUT_SECS: u64 = 10;

    // Stop accepting new connections
    server_handle.shutdown();

    // Give active transfers time to complete
    tracing::info!("Shutting down gracefully ({}s timeout)...", SHUTDOWN_TIMEOUT_SECS);
    tokio::time::sleep(Duration::from_secs(SHUTDOWN_TIMEOUT_SECS)).await;

    // Clean up session maps
    cleanup_sessions(&app_state).await;

    tracing::info!("Server shutdown complete");
    Ok(())
}

/// Clean up all active sessions, triggering Drop cleanup for incomplete transfers
async fn cleanup_sessions(state: &AppState) {
    let receive_count = state.receive_sessions.len();
    let send_count = state.send_sessions.len();

    if receive_count > 0 || send_count > 0 {
        tracing::info!(
            "Cleaning up {} receive session(s) and {} send session(s)",
            receive_count,
            send_count
        );
    }

    // Clear maps - ChunkStorage::Drop automatically deletes incomplete files
    state.receive_sessions.clear();
    state.send_sessions.clear();

    tracing::debug!("Session cleanup complete");
}

fn direction_to_str(direction: ServerDirection) -> &'static str {
    match direction {
        ServerDirection::Send => "send",
        ServerDirection::Receive => "receive",
    }
}
