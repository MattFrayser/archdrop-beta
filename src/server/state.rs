use axum::Router;
use tokio::sync::watch;
use uuid::Uuid;

use crate::{crypto::EncryptionKey, server::session::Session};

pub enum ServerMode {
    Local,
    Tunnel,
}

pub enum ServerDirection {
    Send,
    Receive,
}

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub session_key: String,
    pub progress_sender: watch::Sender<f64>,
}
impl AppState {
    pub fn new(session: Session, session_key: String, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            session_key,
            progress_sender,
        }
    }
}

// Server config and runtime state
pub struct ServerInstance {
    pub app: Router,
    pub token: String,
    pub session_key: String,
    pub nonce: String,
    pub progress_sender: watch::Sender<f64>,
    pub progress_receiver: watch::Receiver<f64>,
    pub display_name: String,
}

impl ServerInstance {
    pub fn new(app: Router, display_name: String, nonce: String) -> Self {
        let session_key = EncryptionKey::new();
        let (progress_sender, progress_receiver) = watch::channel(0.0);

        Self {
            app,
            token: Uuid::new_v4().to_string(),
            session_key: session_key.to_base64(),
            nonce,
            progress_sender,
            progress_receiver,
            display_name,
        }
    }
}
