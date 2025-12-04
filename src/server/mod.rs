pub mod auth;
pub mod modes;
pub mod session;
pub mod state;
pub mod utils;
pub mod web;

use crate::server::session::Session;
use crate::server::state::AppState;
use crate::transfer::{manifest::Manifest, receive, send};
use crate::types::{EncryptionKey, Nonce};
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

// Server config and runtime state
pub struct ServerInstance {
    pub app: Router,
    pub session: Session,
    pub display_name: String,
    pub progress_sender: watch::Sender<f64>,
}

impl ServerInstance {
    pub fn new(
        app: Router,
        session: Session,
        display_name: String,
        progress_sender: watch::Sender<f64>,
    ) -> Self {
        Self {
            app,
            session,
            display_name,
            progress_sender,
        }
    }

    pub fn progress_receiver(&self) -> watch::Receiver<f64> {
        self.progress_sender.subscribe()
    }
}

async fn start_server(
    server: ServerInstance,
    app_state: AppState,
    mode: ServerMode,
    direction: ServerDirection,
    nonce: Nonce,
) -> Result<u16> {
    match mode {
        ServerMode::Local => modes::start_https(server, app_state, direction, nonce).await,
        ServerMode::Tunnel => modes::start_tunnel(server, app_state, direction, nonce).await,
    }
}

//----------------
// SEND SERVER
//---------------
pub async fn start_send_server(manifest: Manifest, mode: ServerMode) -> Result<u16> {
    // Generate crypto keys
    let session_key = EncryptionKey::new();
    let nonce = Nonce::new();

    // TUI display
    let display_name = if manifest.files.len() == 1 {
        manifest.files[0].name.clone()
    } else {
        format!("{} files", manifest.files.len())
    };

    // Send specific session
    let session = session::Session::new_send(manifest.clone(), session_key);
    let (progress_sender, _) = tokio::sync::watch::channel(0.0);

    let state = AppState::new_send(session.clone(), progress_sender.clone());

    // Create axium router
    // Note: More specific routes must come before less specific ones
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/send/:token/manifest", get(send::manifest_handler))
        .route(
            "/send/:token/:file_index/chunk/:chunk_index",
            get(send::send_handler),
        )
        .route("/send/:token/:file_index/hash", get(send::get_file_hash))
        .route("/send/:token/complete", post(send::complete_download))
        .route("/send/:token", get(web::serve_download_page))
        .route("/download.js", get(web::serve_download_js))
        .route("/styles.css", get(web::serve_shared_css))
        .route("/shared.js", get(web::serve_shared_js))
        .with_state(state.clone());

    let server = ServerInstance::new(app, session, display_name, progress_sender);

    start_server(server, state, mode, ServerDirection::Send, nonce).await
}

//----------------
// RECEIVE SERVER
//----------------
pub async fn start_receive_server(destination: PathBuf, mode: ServerMode) -> Result<u16> {
    // Generate crypto keys
    let session_key = EncryptionKey::new();
    let nonce = Nonce::new();

    // TUI display name
    let display_name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".")
        .to_string();

    // Receive specific session
    let session = session::Session::new_receive(destination.clone(), session_key);
    let (progress_sender, _) = tokio::sync::watch::channel(0.0);

    let state = AppState::new_receive(session.clone(), progress_sender.clone());

    // Create axium router
    // Note: More specific routes must come before less specific ones
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/receive/:token/chunk", post(receive::receive_handler))
        .route("/receive/:token/finalize", post(receive::finalize_upload))
        .route("/receive/:token", get(web::serve_upload_page))
        .route("/receive/:token/complete", post(receive::complete_transfer))
        .route("/upload.js", get(web::serve_upload_js))
        .route("/styles.css", get(web::serve_shared_css))
        .route("/shared.js", get(web::serve_shared_js))
        .with_state(state.clone());

    let server = ServerInstance::new(app, session, display_name, progress_sender);

    start_server(server, state, mode, ServerDirection::Receive, nonce).await
}
