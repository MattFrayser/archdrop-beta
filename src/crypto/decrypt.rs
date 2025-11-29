use crate::crypto::{EncryptionKey, Nonce};
use aes_gcm::{Aes256Gcm, KeyInit};
use aes_gcm::aead::{Aead, generic_array::GenericArray};

pub fn decrypt_chunk_at_position(
    key: &EncryptionKey,
    nonce_base: &Nonce,
    encrypted_data: &[u8],
    counter: u32,
) -> anyhow::Result<Vec<u8>> {
    // Consctruct full 12 byte nonce
    // [7 byte base][4 byte BE counter][1 byte end flag]
    let mut full_nonce = [0u8; 12];
    full_nonce[..7].copy_from_slice(nonce_base.as_bytes());
    full_nonce[7..11].copy_from_slice(&counter.to_be_bytes());
    // end flag left 0 for non final

    let cipher = Aes256Gcm::new(GenericArray::from_slice(key.as_bytes()));
    let nonce_array = GenericArray::from_slice(&full_nonce);

    cipher
        .decrypt(nonce_array, encrypted_data)
        .map_err(|e| anyhow::anyhow!("Decryption failed at counter {}: {:?}", counter, e))
}
