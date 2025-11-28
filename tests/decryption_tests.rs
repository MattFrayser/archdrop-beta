#[cfg(test)]
mod tests {
    use aes_gcm::aead::generic_array::GenericArray;
    use aes_gcm::aead::stream::EncryptorBE32;
    use aes_gcm::Aes256Gcm;
    use archdrop::crypto::{decrypt_chunk_at_position, EncryptionKey, Nonce};

    #[test]
    fn test_counter_synchronization() {
        let key = EncryptionKey::new();
        let nonce = Nonce::new();

        // Encrypt 3 chunks using stream encryptor (like client does)
        let encryptor_key = GenericArray::from_slice(key.as_bytes());
        let encryptor_nonce = GenericArray::from_slice(nonce.as_bytes());
        let mut encryptor = EncryptorBE32::<Aes256Gcm>::new(encryptor_key, encryptor_nonce);

        let chunk1 = b"First chunk data";
        let chunk2 = b"Second chunk data";
        let chunk3 = b"Third chunk data";

        let encrypted1 = encryptor.encrypt_next(chunk1.as_slice()).unwrap();
        let encrypted2 = encryptor.encrypt_next(chunk2.as_slice()).unwrap();
        let encrypted3 = encryptor.encrypt_next(chunk3.as_slice()).unwrap();

        // Decrypt using counter positions (like server does)
        let decrypted1 = decrypt_chunk_at_position(&key, &nonce, &encrypted1, 0).unwrap();
        let decrypted2 = decrypt_chunk_at_position(&key, &nonce, &encrypted2, 1).unwrap();
        let decrypted3 = decrypt_chunk_at_position(&key, &nonce, &encrypted3, 2).unwrap();

        assert_eq!(decrypted1, chunk1);
        assert_eq!(decrypted2, chunk2);
        assert_eq!(decrypted3, chunk3);
    }
}
