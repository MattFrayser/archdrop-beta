use crate::types::Nonce;
use aes_gcm::{aead::Aead, Aes256Gcm};
use anyhow::Result;
use sha2::digest::generic_array::GenericArray;

pub fn decrypt_chunk_at_position(
    cipher: &Aes256Gcm,
    nonce_base: &Nonce,
    encrypted_data: &[u8],
    counter: u32,
) -> Result<Vec<u8>> {
    // Consctruct full 12 byte nonce
    // [7 byte base][4 byte BE counter][1 byte end flag]
    let mut full_nonce = [0u8; 12];
    full_nonce[..7].copy_from_slice(nonce_base.as_bytes());
    full_nonce[7..11].copy_from_slice(&counter.to_be_bytes());
    // end flag left 0 for non final

    let nonce_array = GenericArray::from_slice(&full_nonce);

    cipher
        .decrypt(nonce_array, encrypted_data)
        .map_err(|e| anyhow::anyhow!("Decryption failed at counter {}: {:?}", counter, e))
}

pub fn encrypt_chunk_at_position(
    cipher: &Aes256Gcm,
    nonce_base: &Nonce,
    plaintext: &[u8],
    counter: u32,
) -> Result<Vec<u8>> {
    // Construct Nonce
    // [7 byte base][4 byte counter][1 byte flag]
    let mut full_nonce = [0u8; 12];
    full_nonce[..7].copy_from_slice(nonce_base.as_bytes());
    full_nonce[7..11].copy_from_slice(&counter.to_be_bytes());

    let nonce_array = GenericArray::from_slice(&full_nonce);

    cipher
        .encrypt(nonce_array, plaintext)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))
}
