use super::utils;
use crate::server::state::{ServerDirection, ServerInstance};
use crate::tunnel::CloudflareTunnel;
use crate::ui::{output, qr};
use anyhow::{Context, Result};
use std::net::SocketAddr;

pub async fn start_https(server: ServerInstance, direction: ServerDirection) -> Result<u16> {
    let spinner = output::spinner("Starting local HTTPS server...");
    // local Ip and Certs
    let local_ip = utils::get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let tls_config = utils::generate_cert(&local_ip)
        .await
        .context("Failed to generate TLS certificate")?;

    // Bind to random port on all interfaces
    let addr = SocketAddr::from(([0, 0, 0, 0], 0));
    let std_listener = std::net::TcpListener::bind(addr)?;
    std_listener
        .set_nonblocking(true)
        .context("Failed to set listener to non-blocking mode")?;
    let port = std_listener.local_addr()?.port();

    // Spawn HTTPS server in background
    let server_handle = axum_server::Handle::new();
    let handle_clone = server_handle.clone();

    tokio::spawn(async move {
        if let Err(e) = axum_server::from_tcp_rustls(std_listener, tls_config)
            .handle(handle_clone)
            .serve(server.app.into_make_service())
            .await
        {
            eprintln!("Server error: {}", e);
        }
    });

    spinner.set_message(format!("Waiting for server on port {}...", port));

    // Wait for server to be ready
    utils::wait_for_server_ready(port, 5, true)
        .await
        .context("Server failed to become ready")?;

    output::finish_spinner_success(&spinner, &format!("Server ready on port {}", port));

    let service = match direction {
        ServerDirection::Send => "send",
        ServerDirection::Receive => "receive",
    };

    let url = format!(
        "https://{}:{}/{}/{}#key={}&nonce={}",
        local_ip, port, service, server.token, server.session_key, server.nonce
    );
    println!("{}", url);

    // Spawn TUI and get handle
    let qr_code = qr::generate_qr(&url)?;
    let tui_handle = utils::spawn_tui(
        server.progress_receiver,
        server.display_name,
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

pub async fn start_tunnel(server: ServerInstance, direction: ServerDirection) -> Result<u16> {
    // Start local HTTP
    let spinner = output::spinner("Starting local server...");

    // Bind to random port on all interfaces
    let addr = SocketAddr::from(([0, 0, 0, 0], 0));
    let std_listener = std::net::TcpListener::bind(addr)?;
    std_listener
        .set_nonblocking(true)
        .context("Failed to set listener to non-blocking mode")?;
    let port = std_listener.local_addr()?.port();

    // Spawn HTTP server in background
    let server_handle = axum_server::Handle::new();
    let handle_clone = server_handle.clone();

    tokio::spawn(async move {
        if let Err(e) = axum_server::from_tcp(std_listener)
            .handle(handle_clone)
            .serve(server.app.into_make_service())
            .await
        {
            eprintln!("Server error: {}", e);
        }
    });

    spinner.set_message(format!("Waiting for server on port {}...", port));

    // Wait for server to be ready before starting tunnel
    utils::wait_for_server_ready(port, 5, false)
        .await
        .context("Server failed to become ready")?;
    output::finish_spinner_success(&spinner, &format!("Server ready on port {}", port));

    // Start tunnel
    let tunnel = CloudflareTunnel::start(port)
        .await
        .context("Failed to establish Cloudflare tunnel")?;

    let service = match direction {
        ServerDirection::Send => "send",
        ServerDirection::Receive => "receive",
    };

    let url = format!(
        "{}/{}/{}#key={}&nonce={}",
        tunnel.url(),
        service,
        server.token,
        server.session_key,
        server.nonce
    );
    println!("{}", url);

    // Spawn TUI and get handle
    let qr_code = qr::generate_qr(&url)?;
    let tui_handle = utils::spawn_tui(
        server.progress_receiver,
        server.display_name,
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
