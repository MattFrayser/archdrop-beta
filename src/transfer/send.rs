use std::io::SeekFrom;
use std::sync::Arc;

use crate::server::auth;
use crate::server::state::AppState;
use crate::transfer::io;
use crate::transfer::manifest::Manifest;
use crate::transfer::receive::ClientIdParam;
use crate::transfer::util::AppError;
use crate::types::Nonce;
use crate::{config, crypto};
use anyhow::{Context, Result};
use axum::extract::Query;
use axum::{
    body::Body,
    extract::{Path, State},
    http::Response,
    Json,
};
use reqwest::header;
use tokio::io::AsyncReadExt;

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

    let file_handles = state
        .file_handles()
        .ok_or_else(|| anyhow::anyhow!("Invalid server mode: not a send server"))?;

    let file_entry = state
        .session
        .get_file(file_index)
        .ok_or_else(|| anyhow::anyhow!("Invalid file index"))?;

    // Calc chunk boundries
    let start = chunk_index as u64 * config::CHUNK_SIZE;
    let end = std::cmp::min(start + config::CHUNK_SIZE, file_entry.size);

    if start >= file_entry.size {
        return Err(anyhow::anyhow!("Chunk index out of bounds").into());
    }

    let chunk_len = (end - start) as usize;

    // Open file on first access
    let file_handle = file_handles
        .entry(file_index)
        .or_insert_with(|| match std::fs::File::open(&file_entry.full_path) {
            Ok(file) => {
                tracing::debug!("Opened file handle for {}", file_entry.name);
                Arc::new(file)
            }
            Err(e) => {
                tracing::error!(
                    "Failed to open file {}: {}",
                    file_entry.full_path.display(),
                    e
                );
                panic!("Failed to open file for sending");
            }
        })
        .clone();

    // read chunk
    let buffer = io::read_chunk_at_position(&file_handle, start, chunk_len)?;

    // encrypt and return
    let file_nonce = Nonce::from_base64(&file_entry.nonce)
        .context(format!("Invalid nonce for file: {}", file_entry.name))?;

    let cipher = state.session.cipher();

    let encrypted =
        crypto::encrypt_chunk_at_position(cipher, &file_nonce, buffer, chunk_index as u32)
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

    // Close all file handles for this session
    if let Some(file_handles) = state.file_handles() {
        let count = file_handles.len();
        file_handles.clear();
        if count > 0 {
            tracing::debug!("Closed {} file handle(s)", count);
        }
    }

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

    let file_entry = state
        .session
        .get_file(file_index)
        .ok_or_else(|| anyhow::anyhow!("Invalid file index"))?;

    let hash = compute_file_hash(&file_entry.full_path).await?;

    Ok(axum::Json(serde_json::json!({
        "sha256": hash
    })))
}

async fn compute_file_hash(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    // Use spawn_blocking for disk I/O
    let path = path.to_owned();

    tokio::task::spawn_blocking(move || {
        use std::io::Read;

        let mut file = std::fs::File::open(&path).context(format!(
            "Failed to open file for hashing: {}",
            path.display()
        ))?;

        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 65536]; // 64 KB chunks

        loop {
            let n = file
                .read(&mut buffer)
                .context("Failed to read file for hashing")?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        Ok::<String, anyhow::Error>(hex::encode(hasher.finalize()))
    })
    .await
    .context("Hash computation task panicked")?
}
