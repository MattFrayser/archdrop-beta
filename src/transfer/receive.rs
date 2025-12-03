use crate::server::state::AppState;
use crate::transfer::chunk::FileReceiveState;
use crate::transfer::storage::ChunkStorage;
use crate::transfer::util::{hash_path, validate_path, AppError};
use crate::types::Nonce;
use anyhow::{Context, Result};
use axum::extract::{Multipart, Path, Query, State};
use axum::Json;
use serde_json::{json, Value};

#[derive(serde::Deserialize)]
pub struct ClientIdParam {
    #[serde(rename = "clientId")]
    pub client_id: String,
}

pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    // Parse upload
    let chunk = parse_chunk_upload(multipart).await?;
    let client_id = &chunk.client_id;

    // Get or create session
    let file_id = hash_path(&chunk.relative_path);

    // check token on first chunk of a new file
    let is_new_file = !state.receive_sessions.contains_key(&file_id);

    if is_new_file && chunk.chunk_index == 0 {
        // First chunk of a new file - need either initial claim or active session
        if !state.session.claim(&token, client_id) {
            return Err(anyhow::anyhow!("Invalid token").into());
        }
    } else {
        // For other chunks, check if active
        if !state.session.is_active(&token, client_id) {
            return Err(anyhow::anyhow!("Invalid or inactive session").into());
        }
    }

    // Lock receive session
    let session_exits = state.receive_sessions.contains_key(&file_id);

    if !session_exits {
        let destination = state
            .session
            .destination()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Invalid session type"))?;

        // Validate provided path and join to base
        validate_path(&chunk.relative_path).context("Invalid file path")?;
        let dest_path = destination.join(&chunk.relative_path);

        let storage = ChunkStorage::new(dest_path)
            .await
            .context("Failed to create storage")?;

        state.receive_sessions.insert(
            file_id.clone(),
            FileReceiveState {
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
        .ok_or_else(|| anyhow::anyhow!("Invalid session"))?;

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
    let nonce = Nonce::from_base64(&session.nonce)?;

    let cipher = state.session.cipher();

    session
        .storage
        .store_chunk(chunk.chunk_index, chunk.data, cipher, &nonce)
        .await?;

    Ok(Json(json!({
        "success": true,
        "chunk": chunk.chunk_index,
        "total": session.total_chunks,
        "received": session.storage.chunk_count()
    })))
}
pub async fn finalize_upload(
    Path(token): Path<String>,
    Query(params): Query<ClientIdParam>,
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

    let client_id = &params.client_id;
    // Validate token
    if !state.session.is_active(&token, client_id) {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

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

    // Finalize storage
    let computed_hash = session.storage.finalize().await?;

    Ok(axum::Json(json!({
        "success": true,
        "sha256": computed_hash,
    })))
}

pub async fn complete_transfer(
    Path(token): Path<String>,
    Query(params): Query<ClientIdParam>,
    State(state): State<AppState>,
) -> Result<axum::Json<Value>, AppError> {
    state.session.complete(&token, &params.client_id);

    let _ = state.progress_sender.send(100.0);

    Ok(Json(
        json!({"success": true, "message": "Transfer complete"}),
    ))
}

async fn parse_chunk_upload(mut multipart: Multipart) -> anyhow::Result<ChunkUpload> {
    let mut chunk_data = None;
    let mut relative_path = None;
    let mut chunk_index = None;
    let mut total_chunks = None;
    let mut file_size = None;
    let mut nonce = None;
    let mut client_id = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("chunk") => chunk_data = Some(field.bytes().await?.to_vec()),
            Some("relativePath") => relative_path = Some(field.text().await?),
            Some("chunkIndex") => chunk_index = Some(field.text().await?.parse()?),
            Some("totalChunks") => total_chunks = Some(field.text().await?.parse()?),
            Some("fileSize") => file_size = Some(field.text().await?.parse()?),
            Some("nonce") => nonce = Some(field.text().await?),
            Some("clientId") => client_id = Some(field.text().await?),
            _ => {}
        }
    }

    Ok(ChunkUpload {
        data: chunk_data.ok_or_else(|| anyhow::anyhow!("Missing chunk"))?,
        relative_path: relative_path.ok_or_else(|| anyhow::anyhow!("Missing relativePath"))?,
        chunk_index: chunk_index.ok_or_else(|| anyhow::anyhow!("Missing chunkIndex"))?,
        total_chunks: total_chunks.ok_or_else(|| anyhow::anyhow!("Missing totalChunks"))?,
        file_size: file_size.ok_or_else(|| anyhow::anyhow!("Missing fileSize"))?,
        nonce,
        client_id: client_id.ok_or_else(|| anyhow::anyhow!("Missing clientId"))?,
    })
}

pub struct ChunkUpload {
    pub data: Vec<u8>,
    pub relative_path: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub file_size: u64,
    pub nonce: Option<String>,
    pub client_id: String,
}
