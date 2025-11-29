use crate::transfer::manifest::{FileEntry, Manifest};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct Session {
    token: String,
    manifest: Option<Arc<Manifest>>, // send mode
    destination: Option<PathBuf>,    // receive mode
    session_key: String,
    used: Arc<Mutex<bool>>,
}

impl Session {
    pub fn new_send(manifest: Manifest, session_key: String) -> (Self, String) {
        let token = Uuid::new_v4().to_string();
        let store = Self {
            token: token.clone(),
            manifest: Some(Arc::new(manifest)),
            destination: None,
            session_key,
            used: Arc::new(Mutex::new(false)),
        };
        (store, token)
    }

    pub fn new_receive(destination: PathBuf, session_key: String) -> (Self, String) {
        let token = Uuid::new_v4().to_string();
        let store = Self {
            token: token.clone(),
            manifest: None,
            destination: Some(destination),
            session_key,
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

    pub async fn mark_used(&self, token: &str) {
        let mut used = self.used.lock().await;
        if token == self.token && !*used {
            *used = true;
        }
    }

    // check if token exists and is not used (read only)
    pub async fn is_valid(&self, token: &str) -> bool {
        token == self.token && !*self.used.lock().await
    }
}
