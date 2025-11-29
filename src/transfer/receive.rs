use crate::crypto::{EncryptionKey, Nonce};
use crate::server::state::{AppState, ReceiveSession};
use crate::transfer::storage::ChunkStorage;
use crate::transfer::util::{hash_path, AppError};
use anyhow::Result;
use axum::extract::{Multipart, Path, State};
use axum::Json;
use serde_json::{json, Value};
use std::path::PathBuf;

pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    eprintln!("[receive] Chunk upload request for token: {}", token);

    // Check token is valid
    if !state.session.is_valid(&token).await {
        eprintln!("[receive] Invalid token: {}", token);
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    // Parse upload
    let chunk = match parse_chunk_upload(multipart).await {
        Ok(c) => {
            eprintln!(
                "[receive] Parsed chunk {} for file: {}",
                c.chunk_index, c.relative_path
            );
            c
        }
        Err(e) => {
            eprintln!("[receive] Failed to parse chunk upload: {:?}", e);
            return Err(e.into());
        }
    };

    // Get or create session
    let file_id = hash_path(&chunk.relative_path);

    // Get or create session using DashMap (lock-free concurrent access)
    let session_exists = state.receive_sessions.contains_key(&file_id);

    if !session_exists {
        let destination = state
            .session
            .get_destination()
            .expect("No destination set for receive session")
            .clone();
        // calc path
        let dest_path = destination.join(&chunk.relative_path);

        // create storage (now fully async, no blocking)
        let storage = ChunkStorage::new(chunk.file_size, dest_path)
            .await
            .expect("Failed to create chunk storage");

        state.receive_sessions.insert(
            file_id.clone(),
            ReceiveSession {
                storage,
                total_chunks: chunk.total_chunks,
                nonce: chunk.nonce.clone().unwrap_or_default(),
                relative_path: chunk.relative_path.clone(),
                file_size: chunk.file_size,
            },
        );
    }

    let mut session = state
        .receive_sessions
        .get_mut(&file_id)
        .expect("Session should exist");

    // Update nonce if provided (chunk 0 contains the nonce)
    if let Some(ref nonce_str) = chunk.nonce {
        if session.nonce.is_empty() {
            eprintln!("[receive] Setting nonce from chunk {}", chunk.chunk_index);
            session.nonce = nonce_str.clone();
        }
    }

    // Check for duplicates
    if session.storage.has_chunk(chunk.chunk_index) {
        return Ok(axum::Json(json!({
            "success": true,
            "duplicate": true,
            "chunk": chunk.chunk_index,
            "received": session.storage.chunk_count(),
            "total": session.total_chunks,
        })));
    }

    // store chunk
    let session_key = match EncryptionKey::from_base64(state.session.session_key()) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("[receive] Failed to parse session key: {:?}", e);
            return Err(e.into());
        }
    };

    let nonce = match Nonce::from_base64(&session.nonce) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[receive] Failed to parse nonce: {:?}", e);
            return Err(e.into());
        }
    };

    match session
        .storage
        .store_chunk(chunk.chunk_index, chunk.data, &session_key, &nonce)
        .await
    {
        Ok(_) => {
            eprintln!("[receive] Successfully stored chunk {}", chunk.chunk_index);
        }
        Err(e) => {
            eprintln!(
                "[receive] Failed to store chunk {}: {:?}",
                chunk.chunk_index, e
            );
            return Err(e.into());
        }
    }

    // Update progress for TUI
    let progress = (session.storage.chunk_count() as f64 / session.total_chunks as f64) * 100.0;
    let _ = state.progress_sender.send(progress);

    Ok(Json(json!({
        "success": true,
        "chunk": chunk.chunk_index,
        "total": session.total_chunks,
        "received": session.storage.chunk_count()
    })))
}
pub async fn finalize_upload(
    Path(token): Path<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    // Parse relativePath
    let mut relative_path = None;
    while let Some(field) = multipart.next_field().await? {
        if field.name() == Some("relativePath") {
            relative_path = Some(field.text().await?);
            break;
        }
    }
    let relative_path = relative_path.ok_or_else(|| anyhow::anyhow!("Missing relativePath"))?;

    // Validate token
    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    // Get destination from session
    let destination = state
        .session
        .get_destination()
        .ok_or_else(|| anyhow::anyhow!("No destination directory for this session"))?
        .clone();

    // Generate file ID and remove from sessions map
    let file_id = hash_path(&relative_path);

    let (_key, session) = state
        .receive_sessions
        .remove(&file_id)
        .ok_or_else(|| anyhow::anyhow!("No upload session found for file: {}", relative_path))?;

    // Verify all chunks received
    if session.storage.chunk_count() != session.total_chunks {
        return Err(anyhow::anyhow!(
            "Incomplete upload: received {}/{} chunks for {}",
            session.storage.chunk_count(),
            session.total_chunks,
            relative_path
        )
        .into());
    }

    // Calculate final destination path
    let dest_path = destination.join(&relative_path);

    // Validate path to prevent traversal attacks
    let canonical_dest = validate_path(&dest_path, &destination)?;

    // Get encryption parameters
    let session_key = EncryptionKey::from_base64(state.session.session_key())?;
    let file_nonce = Nonce::from_base64(&session.nonce)?;

    // Finalize storage
    // For Memory: decrypts all chunks and writes to disk
    // For DirectWrite: file already written, just verifies and returns hash
    let computed_hash = session
        .storage
        .finalize(
            &canonical_dest,
            &session_key,
            &file_nonce,
            session.total_chunks,
        )
        .await?;

    Ok(axum::Json(json!({
        "success": true,
        "path": relative_path,
        "size": session.file_size,
        "sha256": computed_hash,
    })))
}

pub async fn complete_transfer(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> Result<axum::Json<Value>, AppError> {
    state.session.mark_used(&token).await;

    // Set progress to 100% to signal completion and close TUI
    let _ = state.progress_sender.send(100.0);

    Ok(Json(
        json!({"success": true, "message": "Transfer complete"}),
    ))
}

async fn parse_chunk_upload(mut multipart: Multipart) -> anyhow::Result<ChunkUpload> {
    let mut chunk_data = None;
    let mut relative_path = None;
    let mut file_name = None;
    let mut chunk_index = None;
    let mut total_chunks = None;
    let mut file_size = None;
    let mut nonce = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("chunk") => chunk_data = Some(field.bytes().await?.to_vec()),
            Some("relativePath") => relative_path = Some(field.text().await?),
            Some("fileName") => file_name = Some(field.text().await?),
            Some("chunkIndex") => chunk_index = Some(field.text().await?.parse()?),
            Some("totalChunks") => total_chunks = Some(field.text().await?.parse()?),
            Some("fileSize") => file_size = Some(field.text().await?.parse()?),
            Some("nonce") => nonce = Some(field.text().await?),
            _ => {}
        }
    }

    Ok(ChunkUpload {
        data: chunk_data.ok_or_else(|| anyhow::anyhow!("Missing chunk"))?,
        relative_path: relative_path.ok_or_else(|| anyhow::anyhow!("Missing relativePath"))?,
        file_name: file_name.ok_or_else(|| anyhow::anyhow!("Missing fileName"))?,
        chunk_index: chunk_index.ok_or_else(|| anyhow::anyhow!("Missing chunkIndex"))?,
        total_chunks: total_chunks.ok_or_else(|| anyhow::anyhow!("Missing totalChunks"))?,
        file_size: file_size.ok_or_else(|| anyhow::anyhow!("Missing fileSize"))?,
        nonce,
    })
}

fn validate_path(dest_path: &PathBuf, base: &PathBuf) -> anyhow::Result<PathBuf> {
    let canonical_dest = if dest_path.exists() {
        // Path exists, canonicalize directly
        dest_path.canonicalize()?
    } else {
        // Path doesn't exist yet, canonicalize parent and append filename
        let parent = dest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid path: no parent directory"))?;

        // Create parent directories if needed
        std::fs::create_dir_all(parent)?;

        let canonical_parent = parent.canonicalize()?;
        let file_name = dest_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid path: no filename"))?;

        canonical_parent.join(file_name)
    };

    // Canonicalize base path
    let canonical_base = base.canonicalize()?;

    // Security check: destination must be within base directory
    if !canonical_dest.starts_with(&canonical_base) {
        return Err(anyhow::anyhow!(
            "Path traversal detected: {} is outside of {}",
            canonical_dest.display(),
            canonical_base.display()
        ));
    }

    Ok(canonical_dest)
}

pub struct ChunkUpload {
    pub data: Vec<u8>,
    pub relative_path: String,
    pub file_name: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub file_size: u64,
    pub nonce: Option<String>,
}
