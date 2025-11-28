use aes_gcm::{
    aead::{
        generic_array::GenericArray,
        stream::{DecryptorBE32, EncryptorBE32},
    },
    Aes256Gcm,
};
use base64::{engine::general_purpose, Engine};
use rand::{rngs::OsRng, RngCore};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;
//---------------------------------------
// AES-256-GCM encryption key (32 bytes)
//---------------------------------------
#[derive(Debug, Clone)]
pub struct EncryptionKey([u8; 32]);

impl EncryptionKey {
    pub fn new() -> Self {
        let mut key = [0u8; 32];
        OsRng::default().fill_bytes(&mut key);
        Self(key)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_base64(&self) -> String {
        general_purpose::URL_SAFE_NO_PAD.encode(&self.0)
    }
    pub fn from_base64(b64: &str) -> anyhow::Result<Self> {
        use base64::{engine::general_purpose, Engine};
        let bytes = general_purpose::URL_SAFE_NO_PAD.decode(b64)?;
        if bytes.len() != 32 {
            anyhow::bail!("Invalid key length");
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Self(key))
    }
}

impl Default for EncryptionKey {
    fn default() -> Self {
        Self::new()
    }
}

//---------------------------------------------------------------------
// 7-byte nonce base for AES-GCM stream encryption
// Combined with a 4-byte counter and 1-byte flag to form 12-byte nonce
//---------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct Nonce([u8; 7]);

impl Nonce {
    // Create new random nonce
    pub fn new() -> Self {
        let mut nonce = [0u8; 7];
        OsRng::default().fill_bytes(&mut nonce);
        Self(nonce)
    }

    // Get raw bytes (for creating stream encryptor/decryptor)
    pub fn as_bytes(&self) -> &[u8; 7] {
        &self.0
    }

    // Encode as base64 for URL
    pub fn to_base64(&self) -> String {
        general_purpose::URL_SAFE_NO_PAD.encode(&self.0)
    }

    pub fn from_base64(b64: &str) -> anyhow::Result<Self> {
        use base64::{engine::general_purpose, Engine};
        let bytes = general_purpose::URL_SAFE_NO_PAD.decode(b64)?;
        if bytes.len() != 7 {
            anyhow::bail!("Invalid nonce length");
        }
        let mut nonce = [0u8; 7];
        nonce.copy_from_slice(&bytes);
        Ok(Self(nonce))
    }
}

impl Default for Nonce {
    fn default() -> Self {
        Self::new()
    }
}
//---------------------
// Encryptor
//--------------------

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

    pub fn from_parts(key: EncryptionKey, nonce: Nonce) -> Self {
        Self { key, nonce }
    }
}

impl Default for Encryptor {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EncryptedFileStream {
    file: File,
    encryptor: EncryptorBE32<Aes256Gcm>,
    buffer: [u8; 65536],
    bytes_sent: u64,
    total_size: u64,
    progress_sender: watch::Sender<f64>,
}

impl EncryptedFileStream {
    pub fn new(
        file: File,
        encryptor: EncryptorBE32<Aes256Gcm>,
        total_size: u64,
        progress_sender: watch::Sender<f64>,
    ) -> Self {
        Self {
            file,
            encryptor,
            buffer: [0u8; 65536],
            bytes_sent: 0,
            total_size,
            progress_sender,
        }
    }

    pub async fn read_next_chunk(&mut self) -> Option<Result<Vec<u8>, std::io::Error>> {
        match self.file.read(&mut self.buffer).await {
            // EOF
            Ok(0) => {
                let _ = self.progress_sender.send(100.0);
                None
            }
            Ok(n) => {
                let chunk = &self.buffer[..n];

                //  encrypt chunk
                let encrypted = match self.encryptor.encrypt_next(chunk) {
                    Ok(enc) => enc,
                    Err(e) => {
                        return Some(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Encryption failed: {:?}", e),
                        )))
                    }
                };

                // Frame = [4 byte len][encypted data]
                let len = encrypted.len() as u32;
                let mut framed = len.to_be_bytes().to_vec();
                framed.extend_from_slice(&encrypted);

                // update progress
                self.bytes_sent += n as u64;
                let progress = (self.bytes_sent as f64 / self.total_size as f64) * 100.0;
                let _ = self.progress_sender.send(progress);

                Some(Ok(framed))
            }
            Err(e) => Some(Err(e)),
        }
    }
}
