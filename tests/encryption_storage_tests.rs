#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_chunks_stored_encrypted() {
        // This test verifies chunks are NOT plaintext in /tmp

        // 1. Upload a chunk with known plaintext
        let plaintext = b"SECRET DATA THAT SHOULD BE ENCRYPTED";
        // ... (upload chunk via handler)

        // 2. Read the chunk file from /tmp
        let chunk_file_data = tokio::fs::read("/tmp/archdrop/test-token/test-file/0.chunk")
            .await
            .unwrap();

        // 3. Verify it's NOT plaintext
        assert_ne!(
            &chunk_file_data[..],
            plaintext,
            "Chunk is stored as plaintext! Security violation!"
        );

        // 4. Verify it CAN be decrypted
        // ... (decrypt and verify matches plaintext)
    }
}
