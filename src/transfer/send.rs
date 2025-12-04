use std::io::SeekFrom;

use crate::server::auth;
use crate::server::state::AppState;
use crate::transfer::chunk::FileSendState;
use crate::transfer::manifest::Manifest;
use crate::transfer::receive::ClientIdParam;
use crate::transfer::util::AppError;
use crate::types::Nonce;
use crate::{config::CHUNK_SIZE, crypto::encrypt_chunk_at_position};
use anyhow::{Context, Result};
use axum::extract::Query;
use axum::{
    body::Body,
    extract::{Path, State},
    http::Response,
    Json,
};
use reqwest::header;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};

#[derive(serde::Deserialize)]
pub struct ChunkParams {
    #[serde(rename = "clientId")]
    client_id: String,
}

pub async fn manifest_handler(
    Path(token): Path<String>,
    Query(params): Query<ClientIdParam>,
    State(state): State<AppState>,
) -> Result<Json<Manifest>, AppError> {
    // Session claimed when fetching manifest
    // Manifests holds info about files (sizes, names) only client should see
    auth::claim_or_validate_session(&state.session, &token, &params.client_id)?;

    // Get manifest from session
    let manifest = state
        .session
        .manifest()
        .ok_or_else(|| anyhow::anyhow!("Not a send session"))?;

    Ok(Json(manifest.as_ref().clone()))
}

pub async fn send_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    Query(params): Query<ChunkParams>,
    State(state): State<AppState>,
) -> Result<Response<Body>, AppError> {
    let client_id = &params.client_id;

    // Sessions are claimed by manifest, so just check client
    auth::require_active_session(&state.session, &token, client_id)?;

    let send_sessions = state
        .send_sessions()
        .ok_or_else(|| anyhow::anyhow!("Invalid server mode: not a send server"))?;

    let file_entry = state
        .session
        .get_file(file_index)
        .ok_or_else(|| anyhow::anyhow!("Invalid file index"))?;

    // Calc chunk boundries
    let start = chunk_index as u64 * CHUNK_SIZE;
    let end = std::cmp::min(start + CHUNK_SIZE, file_entry.size);

    if start >= file_entry.size {
        return Err(anyhow::anyhow!("Chunk index out of bounds").into());
    }

    let chunk_len = (end - start) as usize;

    // Initialize session and open file
    let total_chunks = ((file_entry.size + CHUNK_SIZE - 1) / CHUNK_SIZE) as usize;

    send_sessions
        .entry(file_index)
        .or_insert_with(|| FileSendState::new(total_chunks));

    // Open file on first access
    if send_sessions.file_handle.is_none() {
        let file = std::fs::File::open(&file_entry.full_path).context(format!(
            "Failed to open file for sending: {}",
            file_entry.full_path.display()
        ))?;

        tracing::debug!(
            "Opened file handle for {}, total chunks: {}",
            file_entry.name,
            total_chunks
        );

        send_sessions.file_hanle = Some(Arc::new(file));
    }

    let mut reader = BufReader::with_capacity(CHUNK_SIZE as usize * 2, file);
    reader.seek(SeekFrom::Start(start)).await?;

    let chunk_len = (end - start) as usize;
    let mut buffer = vec![0u8; chunk_len];
    reader.read_exact(&mut buffer).await.context(format!(
        "Failed to read chunk {} from file {} (offset {})",
        chunk_index, file_entry.name, start
    ))?;

    // Hash plaintext chunk for verification
    if let Some(mut session) = send_sessions.get_mut(&file_index) {
        session.process_chunk(chunk_index, &buffer);
    }

    let file_nonce = Nonce::from_base64(&file_entry.nonce)
        .context(format!("Invalid nonce for file: {}", file_entry.name))?;

    let cipher = state.session.cipher();

    let encrypted = encrypt_chunk_at_position(cipher, &file_nonce, &buffer, chunk_index as u32)
        .context(format!(
            "Failed to encrypt chunk {} of file {}",
            chunk_index, file_entry.name
        ))?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(encrypted))?)
}

pub async fn complete_download(
    Path(token): Path<String>,
    Query(params): Query<ChunkParams>,
    State(state): State<AppState>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    // Session must be active and owned to complete
    let client_id = &params.client_id;
    auth::require_active_session(&state.session, &token, client_id)?;

    state.session.complete(&token, client_id);

    // Set progress to 100% to signal completion and close TUI
    let _ = state.progress_sender.send(100.0);

    Ok(axum::Json(serde_json::json!({
        "success": true,
        "message": "Download complete"
    })))
}

pub async fn get_file_hash(
    Path((token, file_index)): Path<(String, usize)>,
    Query(params): Query<ChunkParams>,
    State(state): State<AppState>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    let client_id = &params.client_id;
    auth::require_active_session(&state.session, &token, client_id)?;

    let send_sessions = state
        .send_sessions()
        .ok_or_else(|| anyhow::anyhow!("Invalid server mode: not a send server"))?;

    // Fast path: Check cache first
    if let Some(session) = send_sessions.get(&file_index) {
        if let Some(ref hash) = session.finalized_hash {
            return Ok(axum::Json(serde_json::json!({
                "sha256": hash
            })));
        }
    }

    // Slow path: Compute from disk and cache
    let file_entry = state
        .session
        .get_file(file_index)
        .ok_or_else(|| anyhow::anyhow!("Invalid file index"))?;

    let hash = compute_file_hash(&file_entry.full_path).await?;

    // Cache the result
    let total_chunks = ((file_entry.size + CHUNK_SIZE - 1) / CHUNK_SIZE) as usize;

    send_sessions
        .entry(file_index)
        .and_modify(|s| s.finalized_hash = Some(hash.clone()))
        .or_insert_with(|| {
            let mut session = FileSendState::new(total_chunks);
            session.finalized_hash = Some(hash.clone());
            session
        });

    Ok(axum::Json(serde_json::json!({
        "sha256": hash
    })))
}

async fn compute_file_hash(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncReadExt;

    let file = tokio::fs::File::open(path).await?;
    let mut reader = tokio::io::BufReader::with_capacity(65536, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 65536]; // 64 KB for efficient I/O

    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}
