use crate::transfer::manifest::{FileEntry, Manifest};
use crate::types::EncryptionKey;
use aes_gcm::{Aes256Gcm, KeyInit};
use sha2::digest::generic_array::GenericArray;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub enum SessionMode {
    Send { manifest: Arc<Manifest> },
    Receive { destination: PathBuf },
}

#[derive(Clone)]
pub struct Session {
    token: String,
    active: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
    client_id: Arc<Mutex<Option<String>>>,
    session_key: EncryptionKey,
    cipher: Arc<Aes256Gcm>,
    mode: SessionMode,
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
            active: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(AtomicBool::new(false)),
            client_id: Arc::new(Mutex::new(None)),
            session_key,
            cipher,
            mode,
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

        // Try claim
        let claimed_ok = self
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();

        if claimed_ok {
            let mut stored_id = self.client_id.lock().unwrap();
            *stored_id = Some(client_id.to_string());
            true
        } else {
            // check if client who claimed
            let stored_id = self.client_id.lock().unwrap();
            match stored_id.as_ref() {
                Some(id) => id == client_id,
                None => false,
            }
        }
    }

    pub fn is_active(&self, token: &str, client_id: &str) -> bool {
        if token != self.token || !self.active.load(Ordering::Acquire) {
            return false;
        }

        let stored_id = self.client_id.lock().unwrap();
        match stored_id.as_ref() {
            Some(stored_id) => stored_id == client_id,
            None => false,
        }
    }

    pub fn complete(&self, token: &str, client_id: &str) -> bool {
        if !self.is_active(token, client_id) {
            return false;
        }
        self.completed.swap(true, Ordering::AcqRel);
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
