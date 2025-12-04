use crate::crypto::types::EncryptionKey;
use crate::transfer::manifest::{FileEntry, Manifest};
use aes_gcm::{Aes256Gcm, KeyInit};
use sha2::digest::generic_array::GenericArray;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Clone)]
pub enum SessionMode {
    Send { manifest: Manifest },
    Receive { destination: PathBuf },
}

#[derive(Debug, Clone)]
pub enum SessionState {
    Unclaimed,
    Active { client_id: String },
    Completed,
}

pub struct Session {
    token: String,
    session_key: EncryptionKey,
    cipher: Arc<Aes256Gcm>,
    mode: SessionMode,
    state: Arc<RwLock<SessionState>>,
    pub total_chunks: AtomicU64,
    pub chunks_sent: Arc<AtomicU64>,
}

impl Clone for Session {
    fn clone(&self) -> Self {
        Self {
            token: self.token.clone(),
            session_key: self.session_key.clone(),
            cipher: self.cipher.clone(),
            mode: self.mode.clone(),
            state: self.state.clone(),
            total_chunks: AtomicU64::new(self.total_chunks.load(Ordering::SeqCst)),
            chunks_sent: self.chunks_sent.clone(),
        }
    }
}

impl Session {
    pub fn new_send(manifest: Manifest, session_key: EncryptionKey, total_chunks: u64) -> Self {
        Self::new(SessionMode::Send { manifest }, session_key, total_chunks)
    }

    pub fn new_receive(
        destination: PathBuf,
        session_key: EncryptionKey,
        total_chunks: u64,
    ) -> Self {
        Self::new(
            SessionMode::Receive { destination },
            session_key,
            total_chunks,
        )
    }

    pub fn new(mode: SessionMode, session_key: EncryptionKey, total_chunks: u64) -> Self {
        let token = Uuid::new_v4().to_string();

        let cipher = Arc::new(Aes256Gcm::new(GenericArray::from_slice(
            session_key.as_bytes(),
        )));

        Self {
            token,
            session_key,
            cipher,
            mode,
            state: Arc::new(RwLock::new(SessionState::Unclaimed)),
            total_chunks: AtomicU64::new(total_chunks),
            chunks_sent: Arc::new(AtomicU64::new(0)),
        }
    }

    // The method to safely increment the count and return the necessary data
    pub fn increment_sent_chunk(&self) -> (u64, u64) {
        // Increment the atomic counter
        let new_count = self.chunks_sent.fetch_add(1, Ordering::SeqCst) + 1;
        let total = self.total_chunks.load(Ordering::SeqCst);

        // Return the new count and the total session size for calculation
        (new_count, total)
    }

    // Set total chunks (for receive mode when manifest arrives)
    pub fn set_total_chunks(&self, total: u64) {
        self.total_chunks.store(total, Ordering::SeqCst);
    }

    // Increment received chunk counter (reuses chunks_sent for receive mode)
    pub fn increment_received_chunk(&self) -> (u64, u64) {
        let chunks_received = self.chunks_sent.fetch_add(1, Ordering::SeqCst) + 1;
        let total = self.total_chunks.load(Ordering::SeqCst);
        (chunks_received, total)
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn session_key(&self) -> &EncryptionKey {
        &self.session_key
    }

    pub fn cipher(&self) -> &Arc<Aes256Gcm> {
        &self.cipher
    }

    pub fn session_key_b64(&self) -> String {
        self.session_key.to_base64()
    }

    // session lock logic

    // Claims inactive session, creates client_id
    pub fn claim(&self, token: &str, client_id: &str) -> bool {
        if token != self.token {
            return false;
        }

        // Convert for storage in SessionState::Active
        let client_id_owned = client_id.to_string();

        // Try to claim
        let mut state = self.state.write().unwrap();
        match *state {
            SessionState::Unclaimed => {
                *state = SessionState::Active {
                    client_id: client_id_owned,
                };
                true
            }
            _ => false, // Already claimed or in another state, return false
        }
    }

    pub fn is_active(&self, token: &str, client_id: &str) -> bool {
        if token != self.token {
            return false;
        }

        let state = self.state.read().unwrap();
        match &*state {
            SessionState::Active {
                client_id: stored_id,
            } => stored_id == client_id,
            _ => false,
        }
    }

    pub fn complete(&self, token: &str, client_id: &str) -> bool {
        if !self.is_active(token, client_id) {
            return false;
        }

        let mut state = self.state.write().unwrap();
        *state = SessionState::Completed;
        true
    }

    // Mode based Helpers
    pub fn manifest(&self) -> Option<&Manifest> {
        match &self.mode {
            SessionMode::Send { manifest } => Some(manifest),
            _ => None,
        }
    }

    pub fn get_file(&self, index: usize) -> Option<&FileEntry> {
        self.manifest().and_then(|m| m.files.get(index))
    }

    pub fn destination(&self) -> Option<&PathBuf> {
        match &self.mode {
            SessionMode::Receive { destination } => Some(destination),
            _ => None,
        }
    }
}
