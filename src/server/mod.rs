pub mod handlers;
pub mod modes;
pub mod utils;
use crate::crypto::EncryptionKey;
use crate::manifest::Manifest;
use crate::session::Session;
use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use std::path::PathBuf;
use tokio::sync::watch;

pub enum ServerMode {
    Local,
    Tunnel,
}

pub enum ServerDirection {
    Send,
    Receive,
}

#[derive(Clone)]
pub struct SendAppState {
    pub sessions: Session,   // Contains manifest
    pub session_key: String, // For creating encryptors
    pub progress_sender: watch::Sender<f64>,
}

#[derive(Clone)]
pub struct ReceiveAppState {
    pub sessions: Session,   // Contains destination
    pub session_key: String, // For decrypting chunks
    pub progress_sender: watch::Sender<f64>,
}

//----------------
// SEND SERVER
//---------------
pub async fn start_send_server(manifest: Manifest, mode: ServerMode) -> Result<u16> {
    // One session key for transfer
    let session_key = EncryptionKey::new();
    let session_key_b64 = session_key.to_base64();

    // Create send session
    let (sessions, token) = Session::new_send(manifest.clone(), session_key_b64.clone());

    // Progress channel
    let (progress_sender, progress_consumer) = watch::channel(0.0); // make progress channel

    // TUI display name
    let file_name = if manifest.files.len() == 1 {
        manifest.files[0].name.clone()
    } else {
        format!("{} files", manifest.files.len())
    };

    let state = SendAppState {
        sessions,
        session_key: session_key_b64.clone(),
        progress_sender,
    };

    // Create axium router
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/send/:token", get(handlers::serve_download_page))
        .route("/send/:token/manifest", get(handlers::serve_manifest))
        .route("/send/:token/:file_index/data", get(handlers::send_handler))
        .route("/download.js", get(handlers::serve_download_js))
        .route("/crypto.js", get(handlers::serve_crypto_js))
        .with_state(state);

    let server = modes::Server {
        app,
        token,
        key: session_key_b64,
        nonce: String::new(), // Not used with manifest, but keeps struct simple
        progress_consumer,
        file_name,
    };

    match mode {
        ServerMode::Local => modes::start_https(server, ServerDirection::Send).await,
        ServerMode::Tunnel => modes::start_tunnel(server, ServerDirection::Send).await,
    }
}

//----------------
// RECEIVE SERVER
//----------------
pub async fn start_receive_server(destination: PathBuf, mode: ServerMode) -> Result<u16> {
    // One session key for transfer
    let session_key = EncryptionKey::new();
    let session_key_b64 = session_key.to_base64();

    // Create send session
    let (sessions, token) = Session::new_send(destination.clone(), session_key_b64.clone());

    // Progress channel
    let (progress_sender, progress_consumer) = watch::channel(0.0); // make progress channel

    // TUI display name
    let file_name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".")
        .to_string();

    let state = ReceiveAppState {
        sessions,
        session_key: session_key_b64.clone(),
        progress_sender,
    };

    // Create axium router
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/receive/:token", get(handlers::serve_upload_page))
        .route("/receive/:token/chunk", post(handlers::receive_handler))
        .route("/receive/:token/status", post(handlers::chunk_status))
        .route("/receive/:token/finalize", post(handlers::finalize_upload))
        .route("/upload.js", get(handlers::serve_upload_js))
        .route("/crypto.js", get(handlers::serve_crypto_js))
        .with_state(state);

    let server = modes::Server {
        app,
        token,
        key: session_key_b64,
        nonce: String::new(),
        progress_consumer,
        file_name,
    };

    match mode {
        ServerMode::Local => modes::start_https(server, ServerDirection::Receive).await,
        ServerMode::Tunnel => modes::start_tunnel(server, ServerDirection::Receive).await,
    }
}
