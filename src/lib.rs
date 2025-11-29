pub mod crypto;
pub mod server;
pub mod transfer;
pub mod tunnel;
pub mod ui;

pub mod config {
    pub const CHUNK_SIZE: u64 = 1024 * 1024; // 1MB (increased from 64KB for better throughput)
    pub const MEMORY_THRESHOLD: u64 = 100 * 1024 * 1024; //100MB
    pub const TUNNEL_TIME_OUT_SECS: u64 = 30;
    pub const MAX_CONCURRENT_CHUNKS: usize = 8; // Parallel chunk processing limit
}
