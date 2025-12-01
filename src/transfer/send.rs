use std::io::SeekFrom;

use crate::config::CHUNK_SIZE;
use crate::crypto::{EncryptionKey, Nonce};
use crate::server::state::AppState;
use crate::transfer::manifest::Manifest;
use crate::transfer::util::AppError;
use aes_gcm::aead::{generic_array::GenericArray, Aead};
use aes_gcm::{Aes256Gcm, KeyInit};
use anyhow::Result;
use axum::{
    body::Body,
    extract::{Path, State},
    http::Response,
    Json,
};
use reqwest::header;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};

pub async fn manifest_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Manifest>, AppError> {
    // Validate token
    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    // Get manifest from session
    let manifest = state
        .session
        .get_manifest()
        .ok_or_else(|| anyhow::anyhow!("No manifest available"))?;

    Ok(Json(manifest.clone()))
}

pub async fn send_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    State(state): State<AppState>,
) -> Result<Response<Body>, AppError> {
    // validate token and get file path
    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    let file_entry = state
        .session
        .get_file(file_index)
        .ok_or_else(|| anyhow::anyhow!("invalid file index"))?;

    // Calc chunk boundries
    let start = chunk_index as u64 * CHUNK_SIZE;
    let end = std::cmp::min(start + CHUNK_SIZE, file_entry.size);

    if start >= file_entry.size {
        return Err(anyhow::anyhow!("Chunk index out of bounds").into());
    }

    // Open with buffered reading for better performance
    let file = tokio::fs::File::open(&file_entry.full_path).await?;
    let mut reader = BufReader::with_capacity(CHUNK_SIZE as usize * 2, file);
    reader.seek(SeekFrom::Start(start)).await?;

    let chunk_len = (end - start) as usize;
    let mut buffer = vec![0u8; chunk_len];
    reader.read_exact(&mut buffer).await?;

    let session_key = EncryptionKey::from_base64(state.session.session_key())?;
    let file_nonce = Nonce::from_base64(&file_entry.nonce)?;

    // Create cipher once per request
    let cipher = Aes256Gcm::new(GenericArray::from_slice(session_key.as_bytes()));

    let encrypted = encrypt_chunk_at_position(&cipher, &file_nonce, &buffer, chunk_index as u32)?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(encrypted))?)
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

pub async fn complete_download(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    state.session.mark_used(&token).await;

    // Set progress to 100% to signal completion and close TUI
    let _ = state.progress_sender.send(100.0);

    Ok(axum::Json(serde_json::json!({
        "success": true,
        "message": "Download complete"
    })))
}
