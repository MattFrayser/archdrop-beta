pub mod modes;
pub mod session;
pub mod state;
pub mod utils;
pub mod web;

use crate::crypto::{EncryptionKey, Nonce};
use crate::server::session::Session;
use crate::server::state::{AppState, ServerInstance};
use crate::transfer::{manifest::Manifest, receive, send};
use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use std::path::PathBuf;

pub enum ServerMode {
    Local,
    Tunnel,
}

pub enum ServerDirection {
    Send,
    Receive,
}

async fn start_server(
    server: state::ServerInstance,
    mode: ServerMode,
    direction: ServerDirection,
) -> Result<u16> {
    match mode {
        ServerMode::Local => modes::start_https(server, direction).await,
        ServerMode::Tunnel => modes::start_tunnel(server, direction).await,
    }
}

//----------------
// SEND SERVER
//---------------
pub async fn start_send_server(manifest: Manifest, mode: ServerMode) -> Result<u16> {
    // Generate crypto keys
    let session_key = EncryptionKey::new();
    let nonce = Nonce::new();
    let session_key_b64 = session_key.to_base64();
    let nonce_b64 = nonce.to_base64();

    // TUI display name
    let display_name = if manifest.files.len() == 1 {
        manifest.files[0].name.clone()
    } else {
        format!("{} files", manifest.files.len())
    };

    // Send specific session
    let (session, token) = Session::new_send(manifest.clone(), session_key_b64.clone());
    let (progress_sender, _) = tokio::sync::watch::channel(0.0);

    let state = AppState::new_send(session, progress_sender.clone());

    // Create axium router
    // Note: More specific routes must come before less specific ones
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/send/:token/manifest", get(send::manifest_handler))
        .route(
            "/send/:token/:file_index/chunk/:chunk_index",
            get(send::send_handler),
        )
        .route("/send/:token", get(web::serve_download_page))
        .route("/download.js", get(web::serve_download_js))
        .route("/crypto.js", get(web::serve_crypto_js))
        .with_state(state);

    let server = ServerInstance::new(
        app,
        display_name,
        nonce_b64,
        token,
        session_key_b64,
        progress_sender,
    );

    start_server(server, mode, ServerDirection::Send).await
}

//----------------
// RECEIVE SERVER
//----------------
pub async fn start_receive_server(destination: PathBuf, mode: ServerMode) -> Result<u16> {
    // Generate crypto keys
    let session_key = EncryptionKey::new();
    let nonce = Nonce::new();
    let session_key_b64 = session_key.to_base64();
    let nonce_b64 = nonce.to_base64();

    // TUI display name
    let display_name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".")
        .to_string();

    // Receive specific session
    let (session, token) = Session::new_receive(destination.clone(), session_key_b64.clone());
    let (progress_sender, _) = tokio::sync::watch::channel(0.0);

    let state = AppState::new_receive(session, progress_sender.clone());

    // Create axium router
    // Note: More specific routes must come before less specific ones
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/receive/:token/chunk", post(receive::receive_handler))
        .route("/receive/:token/finalize", post(receive::finalize_upload))
        .route("/receive/:token", get(web::serve_upload_page))
        .route("/receive/:token/complete", post(receive::complete_transfer))
        .route("/upload.js", get(web::serve_upload_js))
        .route("/crypto.js", get(web::serve_crypto_js))
        .with_state(state);

    let server = ServerInstance::new(
        app,
        display_name,
        nonce_b64,
        token,
        session_key_b64,
        progress_sender,
    );

    start_server(server, mode, ServerDirection::Receive).await
}
