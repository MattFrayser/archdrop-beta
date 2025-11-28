use crate::crypto::Encryptor;
use crate::session::SessionStore;
use crate::types::{AppError, ChunkMetadata, StatusQuery, hash_path};
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, Response};
use axum::Json;
use futures::stream;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;

#[derive(Clone)]
pub struct AppState {
    pub sessions: SessionStore,
    pub encryptor: Arc<Encryptor>,
    pub progress_sender: watch::Sender<f64>,
}

// ============================================================================
// DOWNLOAD HANDLERS (Send mode)
// ============================================================================

pub async fn download_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    // Validate token and get file path
    let file_path = state
        .sessions
        .validate_and_mark_used(&token)
        .await
        .ok_or_else(|| anyhow::anyhow!("Invalid or expired token"))?;

    // Extract filename
    let filename = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");

    // Open file asynchronously
    let file = File::open(&file_path).await?;

    let encryptor = state.encryptor.create_stream_encryptor();
    let progress_sender = state.progress_sender.clone();

    // Get file size for progress tracking
    let file_metadata = tokio::fs::metadata(&file_path).await?;
    let total_size = file_metadata.len() as f64;
    let bytes_sent = 0u64;

    // Create async stream for chunked upload
    let stream = stream::unfold(
        (
            file,
            encryptor,
            [0u8; 4096],
            bytes_sent,
            total_size,
            progress_sender,
        ),
        |(mut file, mut enc, mut buf, mut bytes_sent, total_size, progress_sender)| async move {
            match file.read(&mut buf).await {
                Ok(0) => {
                    let _ = progress_sender.send(100.0);
                    None
                }
                Ok(n) => {
                    let chunk = &buf[..n];

                    // Encrypt chunk
                    let encrypted = enc.encrypt_next(chunk).ok()?;

                    // Frame format: [4-byte length][encrypted data]
                    let len = encrypted.len() as u32;
                    let mut framed = len.to_be_bytes().to_vec();
                    framed.extend_from_slice(&encrypted);

                    // Update progress
                    bytes_sent += n as u64;
                    let progress = (bytes_sent as f64 / total_size) * 100.0;
                    let _ = progress_sender.send(progress);

                    Some((
                        Ok::<_, std::io::Error>(framed),
                        (file, enc, buf, bytes_sent, total_size, progress_sender),
                    ))
                }
                Err(e) => Some((
                    Err(e),
                    (file, enc, buf, bytes_sent, total_size, progress_sender),
                )),
            }
        },
    );

    println!("Starting stream");
    Ok(Response::builder()
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(Body::from_stream(stream))?)
}

pub async fn serve_download_page() -> Html<&'static str> {
    const HTML: &str = include_str!("../../templates/download/download.html");
    Html(HTML)
}

pub async fn serve_download_js() -> Response {
    const JS: &str = include_str!("../../templates/download/download.js");
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .body(Body::from(JS))
        .unwrap()
}

// ============================================================================
// UPLOAD HANDLERS (Receive mode) - Resumable Chunked Upload
// ============================================================================

/// POST /upload/:token/chunk
/// Receives a single encrypted chunk, decrypts it, saves to temp storage
pub async fn upload_chunk(
    Path(token): Path<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    // Check token is valid (don't mark as used - need for all chunks)
    if !state.sessions.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid or expired token").into());
    }

    // Parse FormData fields
    let mut chunk_data = None;
    let mut relative_path = None;
    let mut file_name = None;
    let mut chunk_index = None;
    let mut total_chunks = None;
    let mut file_size = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("chunk") => chunk_data = Some(field.bytes().await?.to_vec()),
            Some("relativePath") => relative_path = Some(field.text().await?),
            Some("fileName") => file_name = Some(field.text().await?),
            Some("chunkIndex") => chunk_index = Some(field.text().await?.parse()?),
            Some("totalChunks") => total_chunks = Some(field.text().await?.parse()?),
            Some("fileSize") => file_size = Some(field.text().await?.parse()?),
            _ => {}
        }
    }

    // Ensure all required fields present
    let chunk_data = chunk_data.ok_or_else(|| anyhow::anyhow!("Missing chunk data"))?;
    let relative_path = relative_path.ok_or_else(|| anyhow::anyhow!("Missing relativePath"))?;
    let file_name = file_name.ok_or_else(|| anyhow::anyhow!("Missing fileName"))?;
    let chunk_index = chunk_index.ok_or_else(|| anyhow::anyhow!("Missing chunkIndex"))?;
    let total_chunks = total_chunks.ok_or_else(|| anyhow::anyhow!("Missing totalChunks"))?;
    let file_size = file_size.ok_or_else(|| anyhow::anyhow!("Missing fileSize"))?;

    // Create temp directory for this file's chunks
    let file_id = hash_path(&relative_path);
    let chunk_dir = format!("/tmp/archdrop/{}/{}", token, file_id);
    tokio::fs::create_dir_all(&chunk_dir).await?;

    // Decrypt chunk
    let decryptor = state.encryptor.create_stream_decryptor();
    let decrypted = decryptor.decrypt_next(&chunk_data)?;

    // Write chunk to disk
    let chunk_path = format!("{}/{}.chunk", chunk_dir, chunk_index);
    tokio::fs::write(&chunk_path, decrypted).await?;

    // Update metadata (track which chunks received)
    let metadata_path = format!("{}/metadata.json", chunk_dir);
    let mut metadata: ChunkMetadata = if tokio::fs::metadata(&metadata_path).await.is_ok() {
        let json_string = tokio::fs::read_to_string(&metadata_path).await?;
        serde_json::from_str(&json_string)?
    } else {
        ChunkMetadata {
            relative_path: relative_path.clone(),
            file_name,
            total_chunks,
            file_size,
            completed_chunks: HashSet::new(),
        }
    };

    metadata.completed_chunks.insert(chunk_index);
    let json_string = serde_json::to_string_pretty(&metadata)?;
    tokio::fs::write(&metadata_path, json_string).await?;

    Ok(Json(json!({
        "success": true,
        "chunk": chunk_index,
        "completed": metadata.completed_chunks.len(),
        "total": total_chunks
    })))
}

/// GET /upload/:token/status?relativePath=...
/// Returns which chunks are already uploaded (for resume capability)
pub async fn chunk_status(
    Path(token): Path<String>,
    Query(query): Query<StatusQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    // Check token is valid
    if !state.sessions.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid or expired token").into());
    }

    let file_id = hash_path(&query.relative_path);
    let metadata_path = format!("/tmp/archdrop/{}/{}/metadata.json", token, file_id);

    // If no metadata exists, nothing uploaded yet
    if tokio::fs::metadata(&metadata_path).await.is_err() {
        return Ok(Json(json!({
            "completed_chunks": [],
            "total_chunks": 0,
            "relative_path": query.relative_path
        })));
    }

    // Load metadata and return completed chunks
    let json_string = tokio::fs::read_to_string(&metadata_path).await?;
    let metadata: ChunkMetadata = serde_json::from_str(&json_string)?;

    let mut completed: Vec<usize> = metadata.completed_chunks.into_iter().collect();
    completed.sort();

    Ok(Json(json!({
        "completed_chunks": completed,
        "total_chunks": metadata.total_chunks,
        "relative_path": metadata.relative_path
    })))
}

/// POST /upload/:token/finalize
/// Merges all chunks into final file with proper folder structure
pub async fn finalize_upload(
    Path(token): Path<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    // Parse relativePath from form
    let mut relative_path = None;
    while let Some(field) = multipart.next_field().await? {
        if field.name() == Some("relativePath") {
            relative_path = Some(field.text().await?);
            break;
        }
    }
    let relative_path = relative_path.ok_or_else(|| anyhow::anyhow!("Missing relativePath"))?;

    // Mark token as used (upload completing)
    let destination = state
        .sessions
        .validate_and_mark_used(&token)
        .await
        .ok_or_else(|| anyhow::anyhow!("Invalid or expired token"))?;

    let file_id = hash_path(&relative_path);
    let chunk_dir = format!("/tmp/archdrop/{}/{}", token, file_id);
    let metadata_path = format!("{}/metadata.json", chunk_dir);

    // Load metadata
    let json_string = tokio::fs::read_to_string(&metadata_path).await?;
    let metadata: ChunkMetadata = serde_json::from_str(&json_string)?;

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
    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Merge all chunks into final file
    let mut output = tokio::fs::File::create(&dest_path).await?;

    for i in 0..metadata.total_chunks {
        let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
        let chunk_data = tokio::fs::read(&chunk_path).await?;
        output.write_all(&chunk_data).await?;
    }

    output.flush().await?;

    // Clean up temp files
    tokio::fs::remove_dir_all(&chunk_dir).await.ok();

    println!("Finalized: {}", dest_path.display());

    Ok(Json(json!({
        "success": true,
        "path": relative_path,
        "size": metadata.file_size
    })))
}

pub async fn serve_upload_page() -> Html<&'static str> {
    const HTML: &str = include_str!("../../templates/upload/upload.html");
    Html(HTML)
}

pub async fn serve_upload_js() -> Response {
    const JS: &str = include_str!("../../templates/upload/upload.js");
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .body(Body::from(JS))
        .unwrap()
}

pub async fn serve_crypto_js() -> Response {
    const JS: &str = include_str!("../../templates/shared/crypto.js");
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .body(Body::from(JS))
        .unwrap()
}
