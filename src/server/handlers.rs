use crate::crypto::{
    decrypt_chunk_at_position, EncryptedFileStream, EncryptionKey, Encryptor, Nonce,
};
use crate::manifest::Manifest;
use crate::server::{ReceiveAppState, SendAppState};
use axum::body::Body;
use axum::extract::Query;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use futures::stream;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, json, to_string_pretty, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

// converts any error to HTTP response
pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        eprintln!("Error: {:?}", self.0);

        // Return generic 500 to client
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

// Auto-convert any error type into AppError
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

#[derive(Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub relative_path: String,
    pub file_name: String,
    pub total_chunks: usize,
    pub file_size: u64,
    pub completed_chunks: HashSet<usize>,
    pub nonce: String,
}
struct ChunkUpload {
    data: Vec<u8>,
    relative_path: String,
    file_name: String,
    chunk_index: usize,
    total_chunks: usize,
    file_size: u64,
    nonce: Option<String>,
}

#[derive(Deserialize)]
pub struct StatusQuery {
    #[serde(rename = "relativePath")]
    pub relative_path: String,
}

//-------------------
// SEND MODE
//-------------------
pub async fn send_handler(
    Path((token, file_index)): Path<(String, usize)>,
    State(state): State<SendAppState>,
) -> Result<Response, AppError> {
    // validate token and get file path
    let file_entry = state
        .sessions
        .get_file(file_index)
        .ok_or_else(|| anyhow::anyhow!("invalid file index"))?;

    let session_key = EncryptionKey::from_base64(state.sessions.session_key())?;
    let file_nonce = Nonce::from_base64(&file_entry.nonce)?;

    let encryptor = Encryptor::from_parts(session_key, file_nonce);

    // open file asynchronously to not block thread
    let file = File::open(&file_entry.full_path).await?;

    // Async Stream
    let stream_reader = EncryptedFileStream::new(
        file,
        encryptor.create_stream_encryptor(),
        file_entry.size,
        state.progress_sender.clone(),
    );

    let stream = stream::unfold(stream_reader, |mut reader| async move {
        reader
            .read_next_chunk()
            .await
            .map(|result| (result, reader))
    });

    println!("Starting stream");
    Ok(Response::builder()
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", file_entry.name),
        )
        .body(Body::from_stream(stream))?)
}

//-------------------
// RECEIVE MODE
//-------------------
pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<ReceiveAppState>,
    multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    // Check token is valid
    if !state.sessions.is_valid(&token).await {
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

// GET /upload/:token/status?relativePath=...
pub async fn chunk_status(
    Path(token): Path<String>,
    Query(query): Query<StatusQuery>,
    State(state): State<ReceiveAppState>,
) -> Result<axum::Json<Value>, AppError> {
    // Check token is valid
    if !state.sessions.is_valid(&token).await {
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

// POST /upload/:token/finalize
pub async fn finalize_upload(
    Path(token): Path<String>,
    State(state): State<ReceiveAppState>,
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

    if !state.sessions.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    let destination = state
        .sessions
        .get_destination()
        .ok_or_else(|| anyhow::anyhow!("No destination for session"))?
        .clone();

    // mark session used on success
    state.sessions.mark_used().await;
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

//----------
// HELPER
//----------

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
async fn load_or_create_metadata(
    token: &str,
    chunk: &ChunkUpload,
) -> anyhow::Result<ChunkMetadata> {
    let file_id = hash_path(&chunk.relative_path);
    let chunk_dir = format!("/tmp/archdrop/{}/{}", token, file_id);
    tokio::fs::create_dir_all(&chunk_dir).await?;

    let metadata_path = format!("{}/metadata.json", chunk_dir);

    if tokio::fs::metadata(&metadata_path).await.is_ok() {
        let json_string = tokio::fs::read_to_string(&metadata_path).await?;
        Ok(serde_json::from_str(&json_string)?)
    } else {
        let nonce = chunk
            .nonce
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing nonce on first chunk"))?
            .clone();

        Ok(ChunkMetadata {
            relative_path: chunk.relative_path.clone(),
            file_name: chunk.file_name.clone(),
            total_chunks: chunk.total_chunks,
            file_size: chunk.file_size,
            completed_chunks: HashSet::new(),
            nonce,
        })
    }
}

async fn save_encrypted_chunk(
    token: &str,
    file_id: &str,
    chunk_index: usize,
    encrypted_data: &[u8],
) -> anyhow::Result<()> {
    let chunk_path = format!("/tmp/archdrop/{}/{}/{}.chunk", token, file_id, chunk_index);
    tokio::fs::write(&chunk_path, encrypted_data).await?;
    Ok(())
}
async fn update_chunk_metadata(
    token: &str,
    file_id: &str,
    metadata: &mut ChunkMetadata,
    chunk_index: usize,
) -> anyhow::Result<()> {
    metadata.completed_chunks.insert(chunk_index);

    let metadata_path = format!("/tmp/archdrop/{}/{}/metadata.json", token, file_id);
    let json = serde_json::to_string_pretty(metadata)?;
    tokio::fs::write(&metadata_path, json).await?;

    Ok(())
}

pub async fn serve_manifest(
    Path(token): Path<String>,
    State(state): State<SendAppState>,
) -> Result<axum::Json<Manifest>, AppError> {
    if !state.sessions.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid or expired token").into());
    }

    let manifest = state
        .sessions
        .get_manifest()
        .ok_or_else(|| anyhow::anyhow!("No manifest for this session"))?
        .clone();

    Ok(axum::Json(manifest))
}
// hash path for safe directory name
pub fn hash_path(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());

    // Return first 16 chars (64 bits) for shorter directory names
    // astronomically unlikely to collide
    format!("{:x}", hasher.finalize())[..16].to_string()
}
//--------------
// Serve Web
//--------------
pub async fn serve_upload_page() -> Result<Html<&'static str>, StatusCode> {
    // return embedded html to brower
    const HTML: &str = include_str!("../../templates/upload/upload.html");
    Ok(Html(HTML))
}

pub async fn serve_upload_js() -> Response {
    const JS: &str = include_str!("../../templates/upload/upload.js");
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .body(Body::from(JS))
        .unwrap()
}

pub async fn serve_download_page() -> Result<Html<&'static str>, StatusCode> {
    // return embedded html to brower
    const HTML: &str = include_str!("../../templates/download/download.html");
    Ok(Html(HTML))
}

pub async fn serve_download_js() -> Response {
    const JS: &str = include_str!("../../templates/download/download.js");
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
