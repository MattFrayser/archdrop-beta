use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct SessionStore {
    token: String,
    file_path: PathBuf,
    used: Arc<Mutex<bool>>,
}

impl SessionStore {
    pub fn new(file_path: PathBuf) -> (Self, String) {
        let token = Uuid::new_v4().to_string();
        let store = Self {
            token: token.clone(),
            file_path,
            used: Arc::new(Mutex::new(false)),
        };
        (store, token)
    }

    pub async fn validate_and_mark_used(&self, token: &str) -> Option<PathBuf> {
        // Wrong token
        if token != self.token {
            return None;
        }
        //Already used
        let mut used = self.used.lock().await;
        if *used {
            return None;
        }

        *used = true;
        Some(self.file_path.clone())
    }

    // check if token exists and is not used (read only)
    pub async fn is_valid(&self, token: &str) -> bool {
        token == self.token && !*self.used.lock().await
    }
}
