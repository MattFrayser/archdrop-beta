use std::io::SeekFrom;

use crate::server::state::{AppState, ChunkSendSession};
use crate::transfer::manifest::Manifest;
use crate::transfer::util::AppError;
use crate::types::Nonce;
use crate::{config::CHUNK_SIZE, crypto::encrypt_chunk_at_position};
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
    if token != state.session.token() {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    // Get manifest from session
    let send_session = state
        .session
        .as_send()
        .ok_or_else(|| anyhow::anyhow!("Not a send session"))?;

    let manifest = send_session.manifest();

    Ok(Json(manifest.clone()))
}

pub async fn send_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    State(state): State<AppState>,
) -> Result<Response<Body>, AppError> {
    // Claim token only once on first chunk
    if file_index == 0 && chunk_index == 0 {
        if !state.session.claim(&token) {
            return Err(anyhow::anyhow!("Session already claimed").into());
        }
    } else if !state.session.is_active(&token) {
        return Err(anyhow::anyhow!("Invalid session").into());
    }

    let send_session = state
        .session
        .as_send()
        .ok_or_else(|| anyhow::anyhow!("Not a send session"))?;

    let file_entry = send_session
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

    // Hash chunk for integrity verification
    let total_chunks = ((file_entry.size + CHUNK_SIZE - 1) / CHUNK_SIZE) as usize;
    let mut chunk_send_session = state
        .send_sessions
        .entry(file_index)
        .or_insert_with(|| ChunkSendSession::new(total_chunks));
    chunk_send_session.process_chunk(chunk_index, &buffer);

    let file_nonce = Nonce::from_base64(&file_entry.nonce)?;
    let cipher = state.session.cipher();

    let encrypted = encrypt_chunk_at_position(cipher, &file_nonce, &buffer, chunk_index as u32)?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(encrypted))?)
}

pub async fn complete_download(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    if !state.session.is_active(&token) {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    state.session.complete(&token);

    // Set progress to 100% to signal completion and close TUI
    let _ = state.progress_sender.send(100.0);

    Ok(axum::Json(serde_json::json!({
        "success": true,
        "message": "Download complete"
    })))
}

pub async fn get_file_hash(
    Path((token, file_index)): Path<(String, usize)>,
    State(state): State<AppState>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    if !state.session.is_active(&token) {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    let chunk_send_session = state
        .send_sessions
        .get(&file_index)
        .ok_or_else(|| anyhow::anyhow!("No hash available yet"))?;

    let hash = chunk_send_session
        .finalized_hash
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Download not complete"))?;

    Ok(axum::Json(serde_json::json!({
        "sha256": hash
    })))
}
