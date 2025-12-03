use super::utils;
use crate::server::state::AppState;
use crate::server::{ServerDirection, ServerInstance};
use crate::tunnel::CloudflareTunnel;
use crate::types::Nonce;
use crate::ui::{output, qr};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

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

    run_session(
        server_handle,
        app_state,
        None,
        display_name,
        progress_receiver,
        url,
        service,
    )
    .await?;
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

    run_session(
        server_handle,
        app_state,
        Some(tunnel),
        display_name,
        progress_receiver,
        url,
        service,
    )
    .await?;

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
    mut tunnel: Option<CloudflareTunnel>,
    display_name: String,
    progress_receiver: tokio::sync::watch::Receiver<f64>,
    url: String,
    service: &str,
) -> Result<()> {
    //  Status and Shutdown Channels
    let (status_sender, status_receiver) = tokio::sync::watch::channel(None);
    let shutdown_init = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown_init.clone();
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);
    let tx_for_task = shutdown_tx.clone();

    // Spawn TUI
    let qr_code = qr::generate_qr(&url)?;
    let mut tui_handle = utils::spawn_tui(
        progress_receiver,
        display_name,
        qr_code,
        service == "upload",
        status_receiver,
    );

    // Spawn Ctrl+C handler with two-stage loop
    let ctrl_c_task = tokio::spawn(async move {
        loop {
            // Wait for Ctrl+C signal
            if tokio::signal::ctrl_c().await.is_err() {
                break;
            }

            if shutdown_clone.load(Ordering::Acquire) {
                // Second Ctrl+C -> force exit
                std::process::exit(1);
            } else {
                // First Ctrl+C -> initiate graceful shutdown
                shutdown_clone.store(true, Ordering::Release);
                // Send signal to the main loop.
                let _ = tx_for_task.send(());

                // loop here to wait for the second signal
            }
        }
    });

    // Wait for TUI to complete OR first Ctrl+C
    let shutdown_requested = tokio::select! {
        _ = &mut tui_handle => {
            tracing::info!("Transfer completed successfully");
            false  // Normal completion
        }
            // Signal received from Ctrl+C task
        _ = shutdown_rx.recv() => {
            tracing::info!("Shutdown requested via Ctrl+C");
            true  // Ctrl+C
        }
    };

    // shutdown warning msg
    if shutdown_requested {
        let active_count = count_active_transfers(&app_state);

        if active_count > 0 {
            // Send the warning to the TUI to be displayed
            let _ = status_sender.send(Some(format!(
                "Warning: {} transfer(s) in progress - Press Ctrl+C again to force quit",
                active_count
            )));
        }
    }

    // stop potential UI block
    // confirm termination
    tui_handle.abort();
    let _ = tui_handle.await;

    // Kill tunnel process if it exists
    if let Some(ref mut t) = tunnel {
        tracing::debug!("Killing cloudflared child process...");
        let _ = t.child_process().start_kill();
    }

    //Clean up the Ctrl+C listener task
    ctrl_c_task.abort();
    let _ = ctrl_c_task.await;

    shutdown(server_handle, app_state, shutdown_init, status_sender).await?;

    Ok(())
}

//==========
// SHUTDOWN
//==========
enum ShutdownResult {
    Completed,
    Forced,
}

async fn shutdown(
    server_handle: axum_server::Handle,
    app_state: AppState,
    force_exit: std::sync::Arc<std::sync::atomic::AtomicBool>,
    status_sender: tokio::sync::watch::Sender<Option<String>>,
) -> Result<()> {
    // Stop accepting new connections
    server_handle.shutdown();
    tracing::info!("Server stopped accepting new connections");

    // Wait for active transfers to complete
    let result = wait_for_transfers(&app_state, force_exit, status_sender.clone()).await;

    // Clear status message before final cleanup
    let _ = status_sender.send(None);

    match result {
        ShutdownResult::Completed => {
            tracing::info!("All transfers completed successfully");
        }
        ShutdownResult::Forced => {
            let remaining = count_active_transfers(&app_state);
            tracing::warn!("Forced shutdown with {} pending transfers", remaining);
        }
    }

    // Clean up sessions
    cleanup_sessions(&app_state).await;
    tracing::info!("Server shutdown complete");

    Ok(())
}

async fn wait_for_transfers(
    state: &AppState,
    force_exit: std::sync::Arc<std::sync::atomic::AtomicBool>,
    status_sender: tokio::sync::watch::Sender<Option<String>>,
) -> ShutdownResult {
    use std::sync::atomic::Ordering;

    let mut last_count = count_active_transfers(state);

    loop {
        // Check force exit
        if force_exit.load(Ordering::Acquire) {
            return ShutdownResult::Forced;
        }

        // all transfers complete
        let current_count = count_active_transfers(state);
        if current_count == 0 {
            return ShutdownResult::Completed;
        }

        // Show progress if count changed
        if current_count != last_count {
            tracing::info!("{} transfer(s) remaining...", current_count);
            let _ = status_sender.send(Some(format!(
                "{} transfer(s) remaining - Press Ctrl+C to force quit",
                current_count
            )));
            last_count = current_count;
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
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

fn count_active_transfers(state: &AppState) -> usize {
    state.receive_sessions.len() + state.send_sessions.len()
}

fn direction_to_str(direction: ServerDirection) -> &'static str {
    match direction {
        ServerDirection::Send => "send",
        ServerDirection::Receive => "receive",
    }
}
