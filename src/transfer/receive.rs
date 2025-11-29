use crate::crypto::{decrypt::decrypt_chunk_at_position, EncryptionKey, Nonce};
use crate::server::state::AppState;
use crate::transfer::{
    chunk::{
        load_or_create_metadata, parse_chunk_upload, save_encrypted_chunk,
        update_chunk_metadata, ChunkMetadata,
    },
    util::{hash_path, AppError, StatusQuery},
};
use axum::extract::{Multipart, Path, Query, State};
use serde_json::{from_str, json, Value};
use tokio::io::AsyncWriteExt;

pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    // Check token is valid
    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    // Parse upload
    let chunk = parse_chunk_upload(multipart).await?;

    // Load or create metadata
    let mut metadata = load_or_create_metadata(&token, &chunk).await?;

    // Save encrypted chunk (no decryption!)
    let file_id = hash_path(&chunk.relative_path);
    save_encrypted_chunk(&token, &file_id, chunk.chunk_index, &chunk.data).await?;

    // Update metadata
    update_chunk_metadata(&token, &file_id, &mut metadata, chunk.chunk_index).await?;

    Ok(axum::Json(json!({
        "success": true,
        "chunk": chunk.chunk_index,
        "completed": metadata.completed_chunks.len(),
        "total": metadata.total_chunks
    })))
}

pub async fn chunk_status(
    Path(token): Path<String>,
    Query(query): Query<StatusQuery>,
    State(state): State<AppState>,
) -> Result<axum::Json<Value>, AppError> {
    // Check token is valid
    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid or expired token").into());
    }

    // get temp dir
    let file_id = hash_path(&query.relative_path);
    let metadata_path = format!("/tmp/archdrop/{}/{}/metadata.json", token, file_id);

    // If no metadata, nothing uploaded yet
    if tokio::fs::metadata(&metadata_path).await.is_err() {
        return Ok(axum::Json(json!({
            "completed_chunks": [],
            "total_chunks": 0,
            "relative_path": query.relative_path
        })));
    }

    // load metadata and return completed chunks
    let data = tokio::fs::read_to_string(&metadata_path).await?;
    let metadata: ChunkMetadata = from_str(&data)?;

    // convert hashset to sorted Vec
    let mut completed: Vec<usize> = metadata.completed_chunks.into_iter().collect();
    completed.sort();

    Ok(axum::Json(json!({
        "completed_chunks": completed,
        "total_chunks": metadata.total_chunks,
        "relative_path": metadata.relative_path
    })))
}

pub async fn finalize_upload(
    Path(token): Path<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    // Parse relativePath from form
    let mut relative_path = None;
    while let Some(field) = multipart.next_field().await? {
        if field.name() == Some("relativePath") {
            relative_path = Some(field.text().await?);
            break;
        }
    }
    let relative_path = relative_path.ok_or_else(|| anyhow::anyhow!("Missing relativePath"))?;

    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    let destination = state
        .session
        .get_destination()
        .ok_or_else(|| anyhow::anyhow!("No destination for session"))?
        .clone();

    // mark session used on success
    state.session.mark_used().await;

    let file_id = hash_path(&relative_path);
    let chunk_dir = format!("/tmp/archdrop/{}/{}", token, file_id);
    let metadata_path = format!("{}/metadata.json", chunk_dir);

    // Load metadata
    let json_string = tokio::fs::read_to_string(&metadata_path).await?;
    let metadata: ChunkMetadata = from_str(&json_string)?;

    // Verify all chunks received
    if metadata.completed_chunks.len() != metadata.total_chunks {
        return Err(anyhow::anyhow!(
            "Missing chunks: received {}, expected {}",
            metadata.completed_chunks.len(),
            metadata.total_chunks
        )
        .into());
    }

    // Create destination with folder structure
    let dest_path = destination.join(&relative_path);

    // block path traversal
    let canonical_dest = if dest_path.exists() {
        dest_path.canonicalize()?
    } else {
        let parent = dest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid path: no parent"))?;
        tokio::fs::create_dir_all(parent).await?;
        let canonical_parent = parent.canonicalize()?;
        canonical_parent.join(dest_path.file_name().unwrap())
    };

    let canonical_base = destination.canonicalize()?;
    if !canonical_dest.starts_with(&canonical_base) {
        return Err(anyhow::anyhow!("Path traversal detected").into());
    }

    // Decrypt and Merge chunks into final file
    let mut output = tokio::fs::File::create(&dest_path).await?;

    // Load encryption key and nonce
    let session_key = EncryptionKey::from_base64(&state.session_key)?;
    let file_nonce = Nonce::from_base64(&metadata.nonce)?;

    // Merge and decrypt chunks sequentially
    for i in 0..metadata.total_chunks {
        let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
        let encrypted_chunk = tokio::fs::read(&chunk_path).await?;

        // Decrypt this chunk using its counter position
        let decrypted = decrypt_chunk_at_position(
            &session_key,
            &file_nonce,
            &encrypted_chunk,
            i as u32, // Counter = chunk index
        )?;

        // Write decrypted data to final file
        output.write_all(&decrypted).await?;
    }

    // Cleanup temp files
    tokio::fs::remove_dir_all(&chunk_dir).await.ok();

    Ok(axum::Json(json!({
        "success": true,
        "path": relative_path,
        "size": metadata.file_size
    })))
}
