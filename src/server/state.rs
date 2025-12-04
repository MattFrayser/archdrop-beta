use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::watch;

use crate::{
    server::session::Session,
    transfer::chunk::{FileReceiveState, FileSendState},
};

#[derive(Clone)]
pub enum TransferStorage {
    Send(Arc<DashMap<usize, FileSendState>>),
    Receive(Arc<DashMap<String, FileReceiveState>>),
}

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub progress_sender: watch::Sender<f64>,
    pub transfers: TransferStorage,
}
impl AppState {
    pub fn new_send(session: Session, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            progress_sender,
            transfers: TransferStorage::Send(Arc::new(DashMap::new())),
        }
    }

    pub fn new_receive(session: Session, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            progress_sender,
            transfers: TransferStorage::Send(Arc::new(DashMap::new())),
        }
    }

    //-- Helper Functions for safe access

    pub fn send_sessions(&self) -> Option<&Arc<DashMap<usize, FileSendState>>> {
        match &self.transfers {
            TransferStorage::Send(sessions) => Some(sessions),
            _ => None,
        }
    }

    pub fn receive_sessions(&self) -> Option<&Arc<DashMap<String, FileReceiveState>>> {
        match &self.transfers {
            TransferStorage::Receive(sessions) => Some(sessions),
            _ => None,
        }
    }

    pub fn transfer_count(&self) -> usize {
        match &self.transfers {
            TransferStorage::Send(sessions) => sessions.len(),
            TransferStorage::Receive(sessions) => sessions.len(),
        }
    }
}
