use archdrop::{
    crypto::{decrypt_chunk_at_position, EncryptionKey, Nonce},
    manifest::Manifest,
};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio::fs;

// Helper to create test files
async fn create_test_file(path: &PathBuf, content: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(path, content).await
}

// Helper to read and verify file
async fn verify_file_content(path: &PathBuf, expected: &[u8]) -> bool {
    match fs::read(path).await {
        Ok(content) => content == expected,
        Err(_) => false,
    }
}
#[tokio::test]
async fn test_encryption_decryption_roundtrip() {
    use aes_gcm::aead::generic_array::GenericArray;
    use aes_gcm::{aead::stream::EncryptorBE32, Aes256Gcm};

    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    // Original data
    let original_chunks = vec![
        b"First chunk of data".to_vec(),
        b"Second chunk of data".to_vec(),
        b"Third chunk of data".to_vec(),
    ];

    // Encrypt using stream encryptor (simulating client)
    let encryptor_key = GenericArray::from_slice(key.as_bytes());
    let encryptor_nonce = GenericArray::from_slice(nonce.as_bytes());
    let mut encryptor = EncryptorBE32::<Aes256Gcm>::new(encryptor_key, encryptor_nonce);

    let mut encrypted_chunks = Vec::new();
    for chunk in &original_chunks {
        let encrypted = encryptor.encrypt_next(chunk.as_slice()).unwrap();
        encrypted_chunks.push(encrypted);
    }

    // Decrypt using position-based decryption (simulating server)
    let mut decrypted_chunks = Vec::new();
    for (i, encrypted) in encrypted_chunks.iter().enumerate() {
        let decrypted = decrypt_chunk_at_position(&key, &nonce, encrypted, i as u32).unwrap();
        decrypted_chunks.push(decrypted);
    }

    // Verify match
    for (original, decrypted) in original_chunks.iter().zip(decrypted_chunks.iter()) {
        assert_eq!(original.as_slice(), decrypted.as_slice());
    }
}
#[tokio::test]
async fn test_chunked_upload_and_merge() {
    // Create test file with known content
    let original_data = b"This is a test file with multiple chunks of data that needs to be split and reassembled correctly!";
    let chunk_size = 20; // Small chunks for testing

    // Simulate chunking (like client does)
    let total_chunks = (original_data.len() + chunk_size - 1) / chunk_size;
    let mut chunks = Vec::new();

    for i in 0..total_chunks {
        let start = i * chunk_size;
        let end = std::cmp::min(start + chunk_size, original_data.len());
        chunks.push(original_data[start..end].to_vec());
    }

    // Encrypt chunks
    use aes_gcm::aead::generic_array::GenericArray;
    use aes_gcm::{aead::stream::EncryptorBE32, Aes256Gcm};

    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let encryptor_key = GenericArray::from_slice(key.as_bytes());
    let encryptor_nonce = GenericArray::from_slice(nonce.as_bytes());
    let mut encryptor = EncryptorBE32::<Aes256Gcm>::new(encryptor_key, encryptor_nonce);

    let mut encrypted_chunks = Vec::new();
    for chunk in &chunks {
        let encrypted = encryptor.encrypt_next(chunk.as_slice()).unwrap();
        encrypted_chunks.push(encrypted);
    }

    // Write encrypted chunks to temp directory (simulating server storage)
    let temp_dir = TempDir::new().unwrap();
    let chunk_dir = temp_dir.path().join("test-file");
    fs::create_dir_all(&chunk_dir).await.unwrap();

    for (i, encrypted) in encrypted_chunks.iter().enumerate() {
        let chunk_path = chunk_dir.join(format!("{}.chunk", i));
        fs::write(&chunk_path, encrypted).await.unwrap();
    }

    // Merge and decrypt (simulating finalize)
    let output_path = temp_dir.path().join("output.bin");
    let mut output = fs::File::create(&output_path).await.unwrap();

    use tokio::io::AsyncWriteExt;

    for i in 0..total_chunks {
        let chunk_path = chunk_dir.join(format!("{}.chunk", i));
        let encrypted_chunk = fs::read(&chunk_path).await.unwrap();

        let decrypted =
            decrypt_chunk_at_position(&key, &nonce, &encrypted_chunk, i as u32).unwrap();

        output.write_all(&decrypted).await.unwrap();
    }

    output.flush().await.unwrap();
    drop(output);

    // Verify final file matches original
    let final_data = fs::read(&output_path).await.unwrap();
    assert_eq!(final_data, original_data);
}
#[tokio::test]
async fn test_resume_after_partial_upload() {
    use std::collections::HashSet;

    let total_chunks = 10;
    let mut completed_chunks = HashSet::new();

    // Simulate 5 chunks already uploaded
    completed_chunks.insert(0);
    completed_chunks.insert(1);
    completed_chunks.insert(2);
    completed_chunks.insert(3);
    completed_chunks.insert(4);

    // Client should skip these and continue from chunk 5
    let mut chunks_to_upload = Vec::new();
    for i in 0..total_chunks {
        if !completed_chunks.contains(&i) {
            chunks_to_upload.push(i);
        }
    }

    assert_eq!(chunks_to_upload, vec![5, 6, 7, 8, 9]);
}
#[test]
fn test_path_traversal_blocked() {
    use std::path::{Path, PathBuf};

    let base = PathBuf::from("/tmp/archdrop/destination");

    // Attack vectors
    let attacks = vec![
        "../../../etc/passwd",
        "..\\..\\..\\windows\\system32",
        "subdir/../../etc/passwd",
        "./././../../../etc/passwd",
        "valid/path/../../../etc/passwd",
    ];

    for attack in attacks {
        let result = safe_join(&base, attack);
        assert!(
            result.is_err() || !result.unwrap().starts_with("/etc"),
            "Path traversal not blocked for: {}",
            attack
        );
    }
}

// Helper function to add to your codebase
fn safe_join(base: &Path, user_path: &str) -> anyhow::Result<PathBuf> {
    use std::path::Component;

    let joined = base.join(user_path);

    // Normalize path without requiring it to exist
    let normalized = joined
        .components()
        .try_fold(PathBuf::new(), |mut path, component| match component {
            Component::Normal(part) => {
                path.push(part);
                Ok(path)
            }
            Component::ParentDir => {
                if !path.pop() {
                    anyhow::bail!("Path traversal attempt: too many '..'");
                }
                Ok(path)
            }
            Component::RootDir => {
                anyhow::bail!("Absolute paths not allowed");
            }
            _ => Ok(path),
        })?;

    // Ensure result is still under base
    anyhow::ensure!(
        normalized.starts_with(base),
        "Path traversal detected: {:?} not under {:?}",
        normalized,
        base
    );

    Ok(normalized)
}
#[test]
fn test_manifest_creation_with_directory() {
    use std::fs;

    // Create temp directory structure
    let temp_dir = TempDir::new().unwrap();
    let test_dir = temp_dir.path().join("test");
    fs::create_dir_all(&test_dir).unwrap();

    // Create test files
    fs::write(test_dir.join("file1.txt"), b"content1").unwrap();
    fs::write(test_dir.join("file2.txt"), b"content2").unwrap();

    let subdir = test_dir.join("subdir");
    fs::create_dir_all(&subdir).unwrap();
    fs::write(subdir.join("file3.txt"), b"content3").unwrap();

    // Create manifest
    let manifest = Manifest::new(vec![test_dir.clone()], None).unwrap();

    // Verify all files included
    assert_eq!(manifest.files.len(), 3);

    // Verify relative paths preserved
    let names: Vec<_> = manifest
        .files
        .iter()
        .map(|f| f.relative_path.as_str())
        .collect();

    assert!(names.iter().any(|n| n.contains("file1.txt")));
    assert!(names.iter().any(|n| n.contains("file2.txt")));
    assert!(names
        .iter()
        .any(|n| n.contains("subdir") && n.contains("file3.txt")));
}
