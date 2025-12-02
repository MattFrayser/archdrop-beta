use std::sync::Arc;

use axum::Router;
use dashmap::DashMap;
use tokio::sync::watch;

use crate::{server::session::Session, transfer::storage::ChunkStorage};

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub progress_sender: watch::Sender<f64>,
    pub receive_sessions: Arc<DashMap<String, ReceiveSession>>,
}
impl AppState {
    pub fn new_send(session: Session, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            progress_sender,
            receive_sessions: Arc::new(DashMap::new()),
        }
    }

    pub fn new_receive(session: Session, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            progress_sender,
            receive_sessions: Arc::new(DashMap::new()),
        }
    }
}

pub struct ReceiveSession {
    pub storage: ChunkStorage,
    pub total_chunks: usize,
    pub nonce: String,
    pub relative_path: String,
    pub file_size: u64,
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

    pub fn build_url(&self, base_url: &str, service: &str) -> String {
        format!(
            "{}/{}/{}#key={}&nonce={}",
            base_url,
            service,
            self.session.token(),
            self.session.session_key_b64(),
            self.session.session_nonce_b64()
        )
    }

    pub fn progress_receiver(&self) -> watch::Receiver<f64> {
        self.progress_sender.subscribe()
    }
}
