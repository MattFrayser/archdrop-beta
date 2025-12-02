use crate::transfer::manifest::{FileEntry, Manifest};
use crate::types::{EncryptionKey, Nonce};
use aes_gcm::{Aes256Gcm, KeyInit};
use sha2::digest::generic_array::GenericArray;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;

/// Private: shared state and crypto for all session types
#[derive(Clone)]
struct SessionCore {
    token: String,
    active: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
    session_key: EncryptionKey,
    /// Session-level nonce used for URL construction only.
    /// Individual files use per-file nonces stored in Manifest (send) or ChunkReceiveSession (receive).
    session_nonce: Nonce,
    cipher: Arc<Aes256Gcm>,
}

impl SessionCore {
    fn new(session_key: EncryptionKey, session_nonce: Nonce) -> (Self, String) {
        let token = Uuid::new_v4().to_string();
        let cipher = Arc::new(Aes256Gcm::new(GenericArray::from_slice(
            session_key.as_bytes(),
        )));

        let core = Self {
            token: token.clone(),
            active: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(AtomicBool::new(false)),
            session_key,
            session_nonce,
            cipher,
        };
        (core, token)
    }

    fn token(&self) -> &str {
        &self.token
    }

    fn session_key(&self) -> &EncryptionKey {
        &self.session_key
    }

    fn cipher(&self) -> &Arc<Aes256Gcm> {
        &self.cipher
    }

    fn session_key_b64(&self) -> String {
        self.session_key.to_base64()
    }

    fn session_nonce_b64(&self) -> String {
        self.session_nonce.to_base64()
    }

    fn claim(&self, token: &str) -> bool {
        if token != self.token {
            return false;
        }
        self.active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn is_active(&self, token: &str) -> bool {
        token == self.token
            && self.active.load(Ordering::Acquire)
            && !self.completed.load(Ordering::Acquire)
    }

    fn complete(&self, token: &str) -> bool {
        if token != self.token || !self.active.load(Ordering::Acquire) {
            return false;
        }
        self.completed.swap(true, Ordering::AcqRel);
        true
    }
}

/// Send-specific session
#[derive(Clone)]
pub struct SendSession {
    core: SessionCore,
    manifest: Arc<Manifest>,
}

impl SendSession {
    pub fn new(manifest: Manifest, session_key: EncryptionKey, session_nonce: Nonce) -> (Self, String) {
        let (core, token) = SessionCore::new(session_key, session_nonce);
        let session = Self {
            core,
            manifest: Arc::new(manifest),
        };
        (session, token)
    }

    // Mode-specific methods - no Option!
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn get_file(&self, index: usize) -> Option<&FileEntry> {
        self.manifest.files.get(index)
    }

    // Delegate shared methods
    pub fn token(&self) -> &str {
        self.core.token()
    }

    pub fn session_key(&self) -> &EncryptionKey {
        self.core.session_key()
    }

    pub fn cipher(&self) -> &Arc<Aes256Gcm> {
        self.core.cipher()
    }

    pub fn session_key_b64(&self) -> String {
        self.core.session_key_b64()
    }

    pub fn session_nonce_b64(&self) -> String {
        self.core.session_nonce_b64()
    }

    pub fn claim(&self, token: &str) -> bool {
        self.core.claim(token)
    }

    pub fn is_active(&self, token: &str) -> bool {
        self.core.is_active(token)
    }

    pub fn complete(&self, token: &str) -> bool {
        self.core.complete(token)
    }
}

/// Receive-specific session
#[derive(Clone)]
pub struct ReceiveSession {
    core: SessionCore,
    destination: PathBuf,
}

impl ReceiveSession {
    pub fn new(destination: PathBuf, session_key: EncryptionKey, session_nonce: Nonce) -> (Self, String) {
        let (core, token) = SessionCore::new(session_key, session_nonce);
        let session = Self {
            core,
            destination,
        };
        (session, token)
    }

    // Mode-specific methods - no Option!
    pub fn destination(&self) -> &PathBuf {
        &self.destination
    }

    // Delegate shared methods
    pub fn token(&self) -> &str {
        self.core.token()
    }

    pub fn session_key(&self) -> &EncryptionKey {
        self.core.session_key()
    }

    pub fn cipher(&self) -> &Arc<Aes256Gcm> {
        self.core.cipher()
    }

    pub fn session_key_b64(&self) -> String {
        self.core.session_key_b64()
    }

    pub fn session_nonce_b64(&self) -> String {
        self.core.session_nonce_b64()
    }

    pub fn claim(&self, token: &str) -> bool {
        self.core.claim(token)
    }

    pub fn is_active(&self, token: &str) -> bool {
        self.core.is_active(token)
    }

    pub fn complete(&self, token: &str) -> bool {
        self.core.complete(token)
    }
}

/// Enum wrapper for storage in AppState
#[derive(Clone)]
pub enum Session {
    Send(SendSession),
    Receive(ReceiveSession),
}

impl Session {
    // Shared accessors via delegation
    pub fn token(&self) -> &str {
        match self {
            Session::Send(s) => s.token(),
            Session::Receive(r) => r.token(),
        }
    }

    pub fn cipher(&self) -> &Arc<Aes256Gcm> {
        match self {
            Session::Send(s) => s.cipher(),
            Session::Receive(r) => r.cipher(),
        }
    }

    pub fn session_key_b64(&self) -> String {
        match self {
            Session::Send(s) => s.session_key_b64(),
            Session::Receive(r) => r.session_key_b64(),
        }
    }

    pub fn session_nonce_b64(&self) -> String {
        match self {
            Session::Send(s) => s.session_nonce_b64(),
            Session::Receive(r) => r.session_nonce_b64(),
        }
    }

    pub fn claim(&self, token: &str) -> bool {
        match self {
            Session::Send(s) => s.claim(token),
            Session::Receive(r) => r.claim(token),
        }
    }

    pub fn is_active(&self, token: &str) -> bool {
        match self {
            Session::Send(s) => s.is_active(token),
            Session::Receive(r) => r.is_active(token),
        }
    }

    pub fn complete(&self, token: &str) -> bool {
        match self {
            Session::Send(s) => s.complete(token),
            Session::Receive(r) => r.complete(token),
        }
    }

    // Type-safe accessors that return concrete types
    pub fn as_send(&self) -> Option<&SendSession> {
        match self {
            Session::Send(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_receive(&self) -> Option<&ReceiveSession> {
        match self {
            Session::Receive(r) => Some(r),
            _ => None,
        }
    }
}
