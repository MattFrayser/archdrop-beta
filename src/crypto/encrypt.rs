use crate::crypto::{EncryptionKey, Nonce};
use aes_gcm::{Aes256Gcm, aead::{generic_array::GenericArray, stream::{DecryptorBE32, EncryptorBE32}}};

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
