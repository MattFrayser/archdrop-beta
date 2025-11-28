use aes_gcm::{
    aead::{
        generic_array::GenericArray,
        stream::{DecryptorBE32, EncryptorBE32},
    },
    Aes256Gcm,
};
use tokio::sync::watch;

use crate::types::{EncryptionKey, Nonce};

use tokio::fs::File;
use tokio::io::AsyncReadExt;

pub struct Encryptor {
    // EncryptorBE32 adds 32-bit counter + 8-bit last-block flag
    // 7 bytes nonce + 4 bytes counter + 1 byte flag = 12 bytes
    key: EncryptionKey,
    nonce: Nonce,
}

impl Encryptor {
    pub fn new() -> Self {
        Self {
            key: EncryptionKey::default(),
            nonce: Nonce::default(),
        }
    }

    pub fn create_stream_encryptor(&self) -> EncryptorBE32<Aes256Gcm> {
        // Convert [u8] to GenericArray<u8, U32> for aes_gcm crate
        let key = GenericArray::from_slice(self.key.as_bytes());
        let nonce = GenericArray::from_slice(self.nonce.as_bytes());

        // EncryptorBE32 handles nonce increment automatically
        // Internally constructs: [7 random bytes][5 bytes for counter]
        EncryptorBE32::new(key, nonce)
    }

    pub fn create_stream_decryptor(&self) -> DecryptorBE32<Aes256Gcm> {
        let key = GenericArray::from_slice(self.key.as_bytes());
        let nonce = GenericArray::from_slice(self.nonce.as_bytes());

        // Decryptor also handles nonce
        // expects same format as EncryptorBE32
        DecryptorBE32::new(key, nonce)
    }
    pub fn get_key_base64(&self) -> String {
        self.key.to_base64()
    }

    pub fn get_nonce_base64(&self) -> String {
        self.nonce.to_base64()
    }
}

impl Default for Encryptor {
    fn default() -> Self {
        Self::new()
    }
}

struct EncryptedFileStream {
    file: File,
    encryptor: EncryptorBE32<Aes256Gcm>,
    buffer: [u8; 4096], // 4Kb buffer
    bytes_sent: u64,
    total_size: u64,
    progress: watch::Sender<f64>,
}

impl EncryptedFileStream {
    async fn next_chunk(&mut self) -> Option<Result<Vec<u8>, std::io::Error>> {
        let n = self.file.read(&mut self.buffer).await.ok()?;

        if n == 0 {
            let _ = self.progress.send(100.0);
            return None;
        }

        let chunk = &self.buffer[..n]; // bytes read

        // encrypt chunk
        let encrypted = self.encryptor.encrypt_next(chunk).ok()?;

        // Frame format for browser parsing
        let len = encrypted.len() as u32;
        let mut framed = len.to_be_bytes().to_vec(); // prefix len
        framed.extend_from_slice(&encrypted); // append encrypted data

        // update progress
        self.bytes_sent += n as u64;
        let progress = (self.bytes_sent as f64 / self.total_size as f64) * 100.0;
        let _ = self.progress.send(progress);

        // return (stream item, state for next)
        Some(Ok(framed))
    }
}
