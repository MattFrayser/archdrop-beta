use crate::transfer::manifest::{FileEntry, Manifest};
use aes_gcm::{Aes256Gcm, KeyInit};
use sha2::digest::generic_array::GenericArray;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct Session {
    token: String,
    active: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
    manifest: Option<Arc<Manifest>>, // send mode
    destination: Option<PathBuf>,    // receive mode
    session_key: String,
    cipher: Arc<Aes256Gcm>,
    used: Arc<Mutex<bool>>,
}

impl Session {
    pub fn new_send(manifest: Manifest, session_key: String) -> (Self, String) {
        let token = Uuid::new_v4().to_string();

        let cipher = Arc::new(Aes256Gcm::new(GenericArray::from_slice(
            session_key.as_bytes(),
        )));

        let store = Self {
            token: token.clone(),
            active: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(AtomicBool::new(false)),
            manifest: Some(Arc::new(manifest)),
            destination: None,
            session_key,
            cipher,
            used: Arc::new(Mutex::new(false)),
        };
        (store, token)
    }

    pub fn new_receive(destination: PathBuf, session_key: String) -> (Self, String) {
        let token = Uuid::new_v4().to_string();

        let cipher = Arc::new(Aes256Gcm::new(GenericArray::from_slice(
            session_key.as_bytes(),
        )));

        let store = Self {
            token: token.clone(),
            active: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(AtomicBool::new(false)),
            manifest: None,
            destination: Some(destination),
            session_key,
            cipher,
            used: Arc::new(Mutex::new(false)),
        };
        (store, token)
    }

    pub fn get_manifest(&self) -> Option<&Manifest> {
        self.manifest.as_ref().map(|m| m.as_ref())
    }

    pub fn get_file(&self, index: usize) -> Option<&FileEntry> {
        self.get_manifest()?.files.get(index)
    }

    pub fn get_destination(&self) -> Option<&PathBuf> {
        self.destination.as_ref()
    }

    pub fn session_key(&self) -> &str {
        &self.session_key
    }

    pub fn cipher(&self) -> &Arc<Aes256Gcm> {
        &self.cipher
    }

    pub fn claim(&self, token: &str) -> bool {
        if token != self.token {
            return false;
        }

        self.active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn is_active(&self, token: &str) -> bool {
        token == self.token
            && self.active.load(Ordering::Acquire)
            && !self.completed.load(Ordering::Acquire)
    }

    pub fn complete(&self, token: &str) -> bool {
        if token != self.token || !self.active.load(Ordering::Acquire) {
            return false;
        }
        self.completed.swap(true, Ordering::AcqRel);
        true
    }
}
