pub mod crypto;
pub mod server;
pub mod transfer;
pub mod tunnel;
pub mod ui;

pub mod config {
    pub const CHUNK_SIZE: u64 = 64 * 1024; // 64KB
    pub const MEMORY_THRESHOLD: u64 = 100 * 1024 * 1024; //100MB
    pub const TUNNEL_TIME_OUT_SECS: u64 = 30;
}
