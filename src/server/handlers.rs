use crate::crypto::Encryptor;
use crate::session::SessionStore;
use axum::body::Body;
use axum::extract::Query;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use futures::stream;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, json, to_string_pretty, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;

#[derive(Clone)]
pub struct AppState {
    pub sessions: SessionStore,
    pub encryptor: Arc<Encryptor>, // Arc for thread-safe shared ownership
    pub progress_sender: watch::Sender<f64>,
}

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

// Add to types.rs (just data structures)

#[derive(Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub relative_path: String,
    pub file_name: String,
    pub total_chunks: usize,
    pub file_size: u64,
    pub completed_chunks: HashSet<usize>,
}

#[derive(Deserialize)]
pub struct StatusQuery {
    #[serde(rename = "relativePath")]
    pub relative_path: String,
}

// Helper: hash path for safe directory name
pub fn hash_path(path: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

//-------------------
// SEND MODE
//-------------------
pub async fn send_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    // validate token and get file path
    let file_path = state
        .sessions
        .validate_and_mark_used(&token)
        .await
        .ok_or_else(|| anyhow::anyhow!("invalid or expired token"))?;

    // Extract filename
    let filename = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download"); // default to generic 'download'

    // open file asynchronously to not block thread
    let file = File::open(&file_path).await?;

    let encryptor = state.encryptor.create_stream_encryptor();

    // clone progress for stream
    let progress_sender = state.progress_sender.clone();

    // file meta data for progress
    let file_metadata = tokio::fs::metadata(&file_path).await?;
    let total_size = file_metadata.len() as f64;
    let bytes_sent = 0u64;

    // Async Stream
    // Create sream form state machine
    let stream = stream::unfold(
        (
            file,
            encryptor,
            [0u8; 65536], // 64KB buffer
            bytes_sent,
            total_size,
            progress_sender,
        ),
        |(mut file, mut enc, mut buf, mut bytes_sent, total_size, progress_sender)| async move {
            //consume buffer
            match file.read(&mut buf).await {
                Ok(0) => {
                    let _ = progress_sender.send(100.0);
                    None
                }
                Ok(n) => {
                    let chunk = &buf[..n]; // bytes read

                    // encrypt chunk
                    let encrypted = enc.encrypt_next(chunk).ok()?; // convert res to Option, end steam on err

                    // Frame format for browser parsing
                    let len = encrypted.len() as u32;
                    let mut framed = len.to_be_bytes().to_vec(); // prefix len
                    framed.extend_from_slice(&encrypted); // append encrypted data

                    // update progress
                    bytes_sent += n as u64;
                    let progress = (bytes_sent as f64 / total_size) * 100.0;
                    let _ = progress_sender.send(progress);

                    // return (stream item, state for next)
                    // Ok wraps body for Body::from_stream
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

//-------------------
// RECEIVE MODE
//-------------------
pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    // Check token is valid
    if !state.sessions.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    // Parse form fields
    let mut chunk_data = None;
    let mut relative_path = None;
    let mut file_name = None;
    let mut chunk_index = None;
    let mut total_chunks = None;
    let mut file_size = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("chunk") => {
                chunk_data = Some(field.bytes().await?.to_vec());
            }
            Some("relativePath") => {
                relative_path = Some(field.text().await?);
            }
            Some("fileName") => {
                file_name = Some(field.text().await?);
            }
            Some("chunkIndex") => {
                chunk_index = Some(field.text().await?.parse()?);
            }
            Some("totalChunks") => {
                total_chunks = Some(field.text().await?.parse()?);
            }
            Some("fileSize") => {
                file_size = Some(field.text().await?.parse()?);
            }
            _ => {}
        }
    }

    // Ensure all required fields
    let chunk_data = chunk_data.ok_or(anyhow::anyhow!("Missing chunk"))?;
    let relative_path = relative_path.ok_or(anyhow::anyhow!("Missing relativePath"))?;
    let file_name = file_name.ok_or(anyhow::anyhow!("Missing fileName"))?;
    let chunk_index = chunk_index.ok_or(anyhow::anyhow!("Missing chunkIndex"))?;
    let total_chunks = total_chunks.ok_or(anyhow::anyhow!("Missing totalChunks"))?;
    let file_size = file_size.ok_or(anyhow::anyhow!("Missing fileSize"))?;

    // Create temp dir for files chunks
    let file_id = hash_path(&relative_path);
    let chunk_dir = format!("/tmp/archdrop/{}/{}", token, file_id);
    tokio::fs::create_dir_all(&chunk_dir).await?;

    // Decrypt chunk
    let mut decryptor = state.encryptor.create_stream_decryptor();
    let decrypted = decryptor
        .decrypt_next(chunk_data.as_slice())
        .map_err(|e| anyhow::anyhow!("Decryption failed: {:?}", e))?;

    // Write chunk to disk
    let chunk_path = format!("{}/{}.chunk", chunk_dir, chunk_index);
    tokio::fs::write(&chunk_path, decrypted).await?;

    // Update metadata to track received chunks
    let metadata_path = format!("{}/metadata.json", chunk_dir);

    // load metadata
    let mut metadata: ChunkMetadata = if tokio::fs::metadata(&metadata_path).await.is_ok() {
        let json_string = tokio::fs::read_to_string(&metadata_path).await?;
        from_str(&json_string)?
    } else {
        ChunkMetadata {
            relative_path: relative_path.clone(),
            file_name,
            total_chunks,
            file_size,
            completed_chunks: HashSet::new(),
        }
    };

    // mark chunk as received
    metadata.completed_chunks.insert(chunk_index);
    let json_string = to_string_pretty(&metadata)?;
    tokio::fs::write(&metadata_path, json_string).await?;

    Ok(axum::Json(json!({
        "success": true,
        "chunk": chunk_index,
        "completed": metadata.completed_chunks.len(),
        "total": total_chunks
    })))
}

// GET /upload/:token/status?relativePath=...
pub async fn chunk_status(
    Path(token): Path<String>,
    Query(query): Query<StatusQuery>,
    State(state): State<AppState>,
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

    // mark token as used
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
    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Merge chunks into final file
    let mut output = tokio::fs::File::create(&dest_path).await?;

    for i in 0..metadata.total_chunks {
        let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
        let chunk_data = tokio::fs::read(&chunk_path).await?;
        output.write_all(&chunk_data).await?;
    }

    output.flush().await?;

    // Cleanup temp files
    tokio::fs::remove_dir_all(&chunk_dir).await.ok();

    Ok(axum::Json(json!({
        "success": true,
        "path": relative_path,
        "size": metadata.file_size
    })))
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
