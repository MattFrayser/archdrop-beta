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
use uuid::Uuid;

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

struct ServerConfig {
    session_key: String,
    token: String,
    progress_sender: watch::Sender<f64>,
    progress_receiver: watch::Receiver<f64>,
    file_display_name: String,
}

fn create_server_config(file_display_name: String) -> ServerConfig {
    let session_key = EncryptionKey::new();
    let (progress_sender, progress_receiver) = watch::channel(0.0);

    ServerConfig {
        session_key: session_key.to_base64(),
        token: Uuid::new_v4().to_string(),
        progress_sender,
        progress_receiver,
        file_display_name,
    }
}

async fn start_server(
    app: Router,
    token: String,
    config: ServerConfig,
    mode: ServerMode,
    direction: ServerDirection,
) -> Result<u16> {
    let server = modes::Server {
        app,
        token,
        key: config.session_key,
        nonce: String::new(),
        progress_consumer: config.progress_receiver,
        file_name: config.file_display_name,
    };

    match mode {
        ServerMode::Local => modes::start_https(server, direction).await,
        ServerMode::Tunnel => modes::start_tunnel(server, direction).await,
    }
}

//----------------
// SEND SERVER
//---------------
pub async fn start_send_server(manifest: Manifest, mode: ServerMode) -> Result<u16> {
    // TUI display name
    let file_display_name = if manifest.files.len() == 1 {
        manifest.files[0].name.clone()
    } else {
        format!("{} files", manifest.files.len())
    };

    let config = create_server_config(file_display_name);

    // Send specific session
    let (sessions, token) = Session::new_send(manifest.clone(), config.session_key.clone());

    let state = SendAppState {
        sessions,
        session_key: config.session_key.clone(),
        progress_sender: config.progress_sender.clone(),
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

    start_server(app, token, config, mode, ServerDirection::Send).await
}

//----------------
// RECEIVE SERVER
//----------------
pub async fn start_receive_server(destination: PathBuf, mode: ServerMode) -> Result<u16> {
    // TUI display name
    let file_display_name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".")
        .to_string();

    let config = create_server_config(file_display_name);

    // Send specific session
    let (sessions, token) = Session::new_receive(destination.clone(), config.session_key.clone());

    let state = ReceiveAppState {
        sessions,
        session_key: config.session_key.clone(),
        progress_sender: config.progress_sender.clone(),
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

    start_server(app, token, config, mode, ServerDirection::Receive).await
}
