use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::watch;

use crate::{
    server::session::Session,
    transfer::chunk::{FileReceiveState, FileSendState},
};

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub progress_sender: watch::Sender<f64>,
    pub receive_sessions: Arc<DashMap<String, FileReceiveState>>,
    pub send_sessions: Arc<DashMap<usize, FileSendState>>,
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
