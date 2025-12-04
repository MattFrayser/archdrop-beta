use crate::transfer::storage::ChunkStorage;
use sha2::{digest::Digest, Sha256};
use std::{collections::HashMap, sync::Arc};

pub struct FileReceiveState {
    pub storage: ChunkStorage,
    pub total_chunks: usize,
    pub nonce: String,
    pub relative_path: String,
    pub file_size: u64,
}

pub struct FileSendState {
    pub file_handle: Option<Arc<std::fs::File>>,
    pub hasher: Sha256,
    pub next_chunk_to_hash: usize,
    pub buffered_chunks: HashMap<usize, Vec<u8>>,
    pub total_chunks: usize,
    pub finalized_hash: Option<String>,
}

impl FileSendState {
    pub fn new(total_chunks: usize) -> Self {
        Self {
            file_handle: None,
            hasher: Sha256::new(),
            next_chunk_to_hash: 0,
            buffered_chunks: HashMap::new(),
            total_chunks,
            finalized_hash: None,
        }
    }

    pub fn process_chunk(&mut self, chunk_index: usize, data: &[u8]) {
        if chunk_index == self.next_chunk_to_hash {
            // Hash immediately
            self.hasher.update(data);
            self.next_chunk_to_hash += 1;

            // Process buffered contiguous chunks
            while let Some(buffered) = self.buffered_chunks.remove(&self.next_chunk_to_hash) {
                self.hasher.update(&buffered);
                self.next_chunk_to_hash += 1;
            }

            // Finalize if complete
            if self.next_chunk_to_hash == self.total_chunks {
                let hash = hex::encode(self.hasher.clone().finalize());
                self.finalized_hash = Some(hash);
                self.buffered_chunks.clear();
            }
        } else {
            // Buffer for later
            self.buffered_chunks.insert(chunk_index, data.to_vec());
        }
    }
}
