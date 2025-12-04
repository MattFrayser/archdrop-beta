//! Public API for starting send/receive servers

use super::{runtime, session};
use crate::crypto::types::{EncryptionKey, Nonce};
use crate::{
    server::{
        routes::{create_receive_router, create_send_router},
        AppState, Session,
    },
    transfer::manifest::Manifest,
};
use anyhow::Result;
use axum::Router;
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

/// Server configuration
pub struct ServerInstance {
    pub app: axum::Router,
    pub session: session::Session,
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

// Generic server helper function
async fn start_server(
    server: ServerInstance,
    app_state: AppState,
    mode: ServerMode,
    direction: ServerDirection,
    nonce: Nonce,
) -> Result<u16> {
    match mode {
        ServerMode::Local => runtime::start_https(server, app_state, direction, nonce).await,
        ServerMode::Tunnel => runtime::start_tunnel(server, app_state, direction, nonce).await,
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
    let app = create_send_router(&state);

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

    let app = create_receive_router(&state);

    let server = ServerInstance::new(app, session, display_name, progress_sender);

    start_server(server, state, mode, ServerDirection::Receive, nonce).await
}
