use archdrop::server::session::{ReceiveSession, SendSession, Session};
use archdrop::transfer::manifest::Manifest;
use archdrop::types::{EncryptionKey, Nonce};
use tempfile::TempDir;

#[tokio::test]
async fn test_send_session_creation() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");
    std::fs::write(&test_file, b"test content").unwrap();

    let manifest = Manifest::new(vec![test_file], None).await.unwrap();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (send_session, token) = SendSession::new(manifest, key, nonce);

    assert!(!token.is_empty(), "Token should not be empty");
    assert_eq!(send_session.token(), token);

    // manifest() returns direct reference, no Option
    let manifest = send_session.manifest();
    assert!(!manifest.files.is_empty(), "Should have files in manifest");
}

#[tokio::test]
async fn test_receive_session_creation() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (receive_session, token) = ReceiveSession::new(dest_path.clone(), key, nonce);

    assert!(!token.is_empty(), "Token should not be empty");
    assert_eq!(receive_session.token(), token);

    // destination() returns direct reference, no Option
    assert_eq!(receive_session.destination(), &dest_path);
}

#[tokio::test]
async fn test_session_enum_wrapper() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");
    std::fs::write(&test_file, b"test content").unwrap();

    let manifest = Manifest::new(vec![test_file], None).await.unwrap();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (send_session, _token) = SendSession::new(manifest, key, nonce);
    let session = Session::Send(send_session);

    // Test type-safe accessors
    assert!(session.as_send().is_some(), "Should be send session");
    assert!(session.as_receive().is_none(), "Should not be receive session");

    // Test shared methods work through enum
    let _cipher = session.cipher();
    let _token = session.token();
}

#[tokio::test]
async fn test_session_claim_valid_token() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (receive_session, token) = ReceiveSession::new(dest_path, key, nonce);

    // First claim should succeed
    assert!(receive_session.claim(&token), "First claim should succeed");

    // Second claim should fail (already active)
    assert!(!receive_session.claim(&token), "Second claim should fail");
}

#[tokio::test]
async fn test_session_claim_invalid_token() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (receive_session, _token) = ReceiveSession::new(dest_path, key, nonce);

    // Wrong token should fail
    assert!(!receive_session.claim("wrong-token"), "Wrong token should fail");
}

#[tokio::test]
async fn test_session_is_active() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (receive_session, token) = ReceiveSession::new(dest_path, key, nonce);

    // Not active before claim
    assert!(!receive_session.is_active(&token), "Should not be active initially");

    // Claim it
    receive_session.claim(&token);

    // Now should be active
    assert!(receive_session.is_active(&token), "Should be active after claim");
}

#[tokio::test]
async fn test_session_complete() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (receive_session, token) = ReceiveSession::new(dest_path, key, nonce);

    // Complete without claim should fail
    assert!(
        !receive_session.complete(&token),
        "Complete should fail before claim"
    );

    // Claim and then complete
    receive_session.claim(&token);
    assert!(
        receive_session.complete(&token),
        "Complete should succeed after claim"
    );

    // Should not be active after complete
    assert!(
        !receive_session.is_active(&token),
        "Should not be active after completion"
    );
}

#[tokio::test]
async fn test_send_session_get_file() {
    let temp_dir = TempDir::new().unwrap();
    let test_file1 = temp_dir.path().join("file1.txt");
    let test_file2 = temp_dir.path().join("file2.txt");
    std::fs::write(&test_file1, b"content1").unwrap();
    std::fs::write(&test_file2, b"content2").unwrap();

    let manifest = Manifest::new(vec![test_file1, test_file2], None)
        .await
        .unwrap();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (send_session, _token) = SendSession::new(manifest, key, nonce);

    // Get files by index
    let file0 = send_session.get_file(0).expect("Should get file 0");
    let file1 = send_session.get_file(1).expect("Should get file 1");

    assert_eq!(file0.index, 0);
    assert_eq!(file1.index, 1);
    assert_eq!(file0.name, "file1.txt");
    assert_eq!(file1.name, "file2.txt");

    // Out of bounds should return None
    assert!(send_session.get_file(999).is_none());
}

#[tokio::test]
async fn test_session_cipher_access() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (receive_session, _token) = ReceiveSession::new(dest_path, key, nonce);

    // Should be able to get cipher
    let _cipher = receive_session.cipher();
    assert!(true, "Cipher should be accessible");
}

#[tokio::test]
async fn test_session_enum_claim_through_wrapper() {
    let temp_dir = TempDir::new().unwrap();
    let dest_path = temp_dir.path().to_path_buf();
    let key = EncryptionKey::new();
    let nonce = Nonce::new();

    let (receive_session, token) = ReceiveSession::new(dest_path, key, nonce);
    let session = Session::Receive(receive_session);

    // Test claim works through enum wrapper
    assert!(session.claim(&token), "First claim should succeed");
    assert!(!session.claim(&token), "Second claim should fail");
    assert!(session.is_active(&token), "Should be active after claim");
}
