pub mod crypto;
pub mod server;
pub mod transfer;
pub mod tunnel;
pub mod ui;

pub mod config {
    pub const CHUNK_SIZE: u64 = 1024 * 1024; // 1MB
    pub const TUNNEL_TIME_OUT_SECS: u64 = 30;
}
