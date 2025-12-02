use super::utils;
use crate::server::state::ServerInstance;
use crate::server::ServerDirection;
use crate::tunnel::CloudflareTunnel;
use crate::ui::{output, qr};
use anyhow::{Context, Result};
use std::net::SocketAddr;

enum Protocol {
    Https,
    Http,
}
pub async fn start_https(server: ServerInstance, direction: ServerDirection) -> Result<u16> {
    let service = direction_to_str(direction);

    // Clone what we need before consuming server
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
        session.session_nonce_b64()
    );

    println!("{}", url);

    run_session(server_handle, display_name, progress_receiver, url, service).await?;
    Ok(port)
}

pub async fn start_tunnel(server: ServerInstance, direction: ServerDirection) -> Result<u16> {
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
        session.session_nonce_b64()
    );
    println!("{}", url);

    run_session(server_handle, display_name, progress_receiver, url, service).await?;

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

    // Graceful shutdown
    server_handle.shutdown();

    Ok(())
}

fn direction_to_str(direction: ServerDirection) -> &'static str {
    match direction {
        ServerDirection::Send => "send",
        ServerDirection::Receive => "receive",
    }
}
