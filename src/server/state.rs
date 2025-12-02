use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use crate::{server::session::Session, transfer::storage::ChunkStorage};

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub progress_sender: watch::Sender<f64>,
    pub receive_sessions: Arc<DashMap<String, ChunkReceiveSession>>,
    pub send_sessions: Arc<DashMap<usize, ChunkSendSession>>,
}
impl AppState {
    pub fn new_send(session: Session, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            progress_sender,
            receive_sessions: Arc::new(DashMap::new()),
            send_sessions: Arc::new(DashMap::new()),
        }
    }

    pub fn new_receive(session: Session, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            progress_sender,
            receive_sessions: Arc::new(DashMap::new()),
            send_sessions: Arc::new(DashMap::new()),
        }
    }
}

pub struct ChunkReceiveSession {
    pub storage: ChunkStorage,
    pub total_chunks: usize,
    pub nonce: String,
    pub relative_path: String,
    pub file_size: u64,
}

pub struct ChunkSendSession {
    pub hasher: Sha256,
    pub next_chunk_to_hash: usize,
    pub buffered_chunks: HashMap<usize, Vec<u8>>,
    pub total_chunks: usize,
    pub finalized_hash: Option<String>,
}

impl ChunkSendSession {
    pub fn new(total_chunks: usize) -> Self {
        Self {
            hasher: Sha256::new(),
            next_chunk_to_hash: 0,
            buffered_chunks: HashMap::new(),
            total_chunks,
            finalized_hash: None,
        }
    }

    pub fn process_chunk(&mut self, chunk_index: usize, data: &[u8]) {
        if chunk_index == self.next_chunk_to_hash {
            // Hash immediately
            self.hasher.update(data);
            self.next_chunk_to_hash += 1;

            // Process buffered contiguous chunks
            while let Some(buffered) = self.buffered_chunks.remove(&self.next_chunk_to_hash) {
                self.hasher.update(&buffered);
                self.next_chunk_to_hash += 1;
            }

            // Finalize if complete
            if self.next_chunk_to_hash == self.total_chunks {
                let hash = hex::encode(self.hasher.clone().finalize());
                self.finalized_hash = Some(hash);
                self.buffered_chunks.clear();
            }
        } else {
            // Buffer for later
            self.buffered_chunks.insert(chunk_index, data.to_vec());
        }
    }
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
