use archdrop::server::session::Session;
use archdrop::transfer::manifest::Manifest;
use archdrop::types::EncryptionKey;
use tempfile::TempDir;

#[tokio::test]
async fn test_session_send_creation() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");
    std::fs::write(&test_file, b"test content").unwrap();

    let manifest = Manifest::new(vec![test_file], None).await.unwrap();
    let key = EncryptionKey::new();

    let (session, token) = Session::new_send(manifest, key);

    assert!(!token.is_empty(), "Token should not be empty");
    assert_eq!(session.token(), token);
    assert!(session.get_manifest().is_some(), "Should have manifest");
    assert!(session.get_destination().is_none(), "Should not have destination");
}

#[tokio::test]
async fn test_session_receive_creation() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();

    let (session, token) = Session::new_receive(dest_path.clone(), key);

    assert!(!token.is_empty(), "Token should not be empty");
    assert_eq!(session.token(), token);
    assert!(session.get_manifest().is_none(), "Should not have manifest");
    assert_eq!(session.get_destination(), Some(&dest_path));
}

#[tokio::test]
async fn test_session_claim_valid_token() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();

    let (session, token) = Session::new_receive(dest_path, key);

    // First claim should succeed
    assert!(session.claim(&token), "First claim should succeed");

    // Second claim should fail (already active)
    assert!(!session.claim(&token), "Second claim should fail");
}

#[tokio::test]
async fn test_session_claim_invalid_token() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();

    let (session, _token) = Session::new_receive(dest_path, key);

    // Wrong token should fail
    assert!(!session.claim("wrong-token"), "Wrong token should fail");
}

#[tokio::test]
async fn test_session_is_active() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();

    let (session, token) = Session::new_receive(dest_path, key);

    // Not active before claim
    assert!(!session.is_active(&token), "Should not be active initially");

    // Claim it
    session.claim(&token);

    // Now should be active
    assert!(session.is_active(&token), "Should be active after claim");
}

#[tokio::test]
async fn test_session_complete() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();

    let (session, token) = Session::new_receive(dest_path, key);

    // Complete without claim should fail
    assert!(!session.complete(&token), "Complete should fail before claim");

    // Claim and then complete
    session.claim(&token);
    assert!(session.complete(&token), "Complete should succeed after claim");

    // Should not be active after complete
    assert!(!session.is_active(&token), "Should not be active after completion");
}

#[tokio::test]
async fn test_session_get_file() {
    let temp_dir = TempDir::new().unwrap();
    let test_file1 = temp_dir.path().join("file1.txt");
    let test_file2 = temp_dir.path().join("file2.txt");
    std::fs::write(&test_file1, b"content1").unwrap();
    std::fs::write(&test_file2, b"content2").unwrap();

    let manifest = Manifest::new(vec![test_file1, test_file2], None)
        .await
        .unwrap();
    let key = EncryptionKey::new();

    let (session, _token) = Session::new_send(manifest, key);

    // Get files by index
    let file0 = session.get_file(0).expect("Should get file 0");
    let file1 = session.get_file(1).expect("Should get file 1");

    assert_eq!(file0.index, 0);
    assert_eq!(file1.index, 1);
    assert_eq!(file0.name, "file1.txt");
    assert_eq!(file1.name, "file2.txt");

    // Out of bounds should return None
    assert!(session.get_file(999).is_none());
}

#[tokio::test]
async fn test_session_cipher_access() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();

    let (session, _token) = Session::new_receive(dest_path, key);

    // Should be able to get cipher
    let _cipher = session.cipher();
    assert!(true, "Cipher should be accessible");
}
