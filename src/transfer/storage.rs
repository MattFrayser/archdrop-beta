// Storage module
// Decides in memory vs use tmp/ folder
// Provides operations for chunk management
// RAII guard is used for cleanups on Error

use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::{
    config::MEMORY_THRESHOLD,
    crypto::{decrypt::decrypt_chunk_at_position, EncryptionKey, Nonce},
};

pub enum ChunkStorage {
    Memory {
        chunks: HashMap<usize, Vec<u8>>,
    },
    DirectWrite {
        output_file: File,
        hasher: Sha256,
        chunks_received: HashSet<usize>,
        guard: PartialFileGuard, // cleanup helper, delete file on error
    },
}

impl ChunkStorage {
    pub async fn new(file_size: u64, dest_path: PathBuf) -> Result<Self> {
        // Size small enough for memory
        if file_size < MEMORY_THRESHOLD {
            Ok(ChunkStorage::Memory {
                chunks: HashMap::new(),
            })
        } else {
            // Create parent dir
            if let Some(parent) = dest_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            let output_file = File::create(&dest_path).await?;
            let guard = PartialFileGuard::new(dest_path);

            Ok(ChunkStorage::DirectWrite {
                output_file,
                hasher: Sha256::new(),
                chunks_received: HashSet::new(),
                guard,
            })
        }
    }
    pub fn has_chunk(&self, chunk_index: usize) -> bool {
        match self {
            ChunkStorage::Memory { chunks } => chunks.contains_key(&chunk_index),
            ChunkStorage::DirectWrite {
                chunks_received, ..
            } => chunks_received.contains(&chunk_index),
        }
    }

    pub fn chunk_count(&self) -> usize {
        match self {
            ChunkStorage::Memory { chunks } => chunks.len(),
            ChunkStorage::DirectWrite {
                chunks_received, ..
            } => chunks_received.len(),
        }
    }

    pub async fn store_chunk(
        &mut self,
        chunk_index: usize,
        encrypted_data: Vec<u8>,
        key: &EncryptionKey,
        nonce: &Nonce,
    ) -> Result<()> {
        match self {
            // In Memory
            ChunkStorage::Memory { chunks } => {
                chunks.insert(chunk_index, encrypted_data);
                Ok(())
            }
            // Write to file
            ChunkStorage::DirectWrite {
                output_file,
                hasher,
                chunks_received,
                ..
            } => {
                // Decrypt chunk
                let decrypted =
                    decrypt_chunk_at_position(key, nonce, &encrypted_data, chunk_index as u32)?;

                // Update hash, write to file, track chunk
                hasher.update(&decrypted);
                output_file.write_all(&decrypted).await?;
                chunks_received.insert(chunk_index);

                Ok(())
            }
        }
    }

    pub async fn finalize(
        self,
        dest_path: &Path,
        key: &EncryptionKey,
        nonce: &Nonce,
        total_chunks: usize,
    ) -> Result<String> {
        match self {
            ChunkStorage::Memory { chunks } => {
                let mut output = File::create(dest_path).await?;
                let mut hasher = Sha256::new();

                // Decrypt chuncks in order
                for i in 0..total_chunks {
                    let encrypted = chunks
                        .get(&i)
                        .ok_or_else(|| anyhow::anyhow!("Missing chunk {}", i))?;

                    // Decrypt, hash, write
                    let decrypted = decrypt_chunk_at_position(key, nonce, encrypted, i as u32)?;
                    hasher.update(&decrypted);
                    output.write_all(&decrypted).await?;
                }

                output.flush().await?;
                Ok(hex::encode(hasher.finalize()))
            }
            ChunkStorage::DirectWrite {
                mut output_file,
                hasher,
                mut guard,
                ..
            } => {
                // file already written on store
                output_file.flush().await?;
                drop(output_file); // close file

                let hash = hex::encode(hasher.finalize());

                guard.disarm(); // success, remove guard

                Ok(hash)
            }
        }
    }
}

pub struct PartialFileGuard {
    path: Option<PathBuf>,
}

impl PartialFileGuard {
    pub fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    // mark on success
    pub fn disarm(&mut self) {
        self.path = None;
    }
}

// auto runs on out of scope
impl Drop for PartialFileGuard {
    fn drop(&mut self) {
        // Check if armed
        if let Some(path) = self.path.take() {
            // spawn cleanup thread
            // drop must be sync so use thread
            std::thread::spawn(move || {
                let _ = std::fs::remove_file(&path);
            });
        }
    }
}
