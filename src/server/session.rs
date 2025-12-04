use crate::crypto::types::EncryptionKey;
use crate::transfer::manifest::{FileEntry, Manifest};
use aes_gcm::{Aes256Gcm, KeyInit};
use sha2::digest::generic_array::GenericArray;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Clone)]
pub enum SessionMode {
    Send { manifest: Arc<Manifest> },
    Receive { destination: PathBuf },
}

#[derive(Debug, Clone)]
pub enum SessionState {
    Unclaimed,
    Active { client_id: String },
    Completed,
}

#[derive(Debug)]
pub enum SessionError {
    InvalidToken,
    AlreadyClaimed,
    NotActive,
}

#[derive(Clone)]
pub struct Session {
    token: String,
    session_key: EncryptionKey,
    cipher: Arc<Aes256Gcm>,
    mode: SessionMode,
    state: Arc<RwLock<SessionState>>,
}

impl Session {
    pub fn new_send(manifest: Manifest, session_key: EncryptionKey) -> Self {
        Self::new(
            SessionMode::Send {
                manifest: Arc::new(manifest),
            },
            session_key,
        )
    }

    pub fn new_receive(destination: PathBuf, session_key: EncryptionKey) -> Self {
        Self::new(SessionMode::Receive { destination }, session_key)
    }

    pub fn new(mode: SessionMode, session_key: EncryptionKey) -> Self {
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
        }
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
    pub fn manifest(&self) -> Option<&Arc<Manifest>> {
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
