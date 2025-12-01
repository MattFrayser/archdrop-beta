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
    pub token: String,
    pub session_key: String,
    pub nonce: String,
    pub progress_sender: watch::Sender<f64>,
    pub progress_receiver: watch::Receiver<f64>,
    pub display_name: String,
}

impl ServerInstance {
    pub fn new(
        app: Router,
        display_name: String,
        nonce: String,
        token: String,
        session_key: String,
        progress_sender: watch::Sender<f64>,
    ) -> Self {
        let progress_receiver = progress_sender.subscribe();

        Self {
            app,
            token,
            session_key,
            nonce,
            progress_sender,
            progress_receiver,
            display_name,
        }
    }
}
