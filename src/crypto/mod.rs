pub mod decrypt;
pub mod encrypt;
pub mod stream;

use base64::{engine::general_purpose, Engine};
use rand::rngs::OsRng;
use rand::RngCore;

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
