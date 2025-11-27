use crate::crypto::Encryptor;
use crate::session::SessionStore;
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{Html, Response};
use futures::stream;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;

#[derive(Clone)]
pub struct AppState {
    pub sessions: SessionStore,
    pub encryptor: Arc<Encryptor>, // Arc = thread-safe shared ownership
    pub progress_sender: watch::Sender<f64>,
}

pub async fn download_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, StatusCode> {
    // validate token and get file path
    let file_path = state
        .sessions
        .validate_and_mark_used(&token)
        .await
        .ok_or(StatusCode::FORBIDDEN)?;

    // Extract filename
    let filename = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download"); // default to generic 'download'

    // open file asynchronously to not block thread
    let file = File::open(&file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?; // Error -> 500

    let encryptor = state.encryptor.create_stream_encryptor();

    // clone progress for stream
    let progress_sender = state.progress_sender.clone();

    // file meta data for progress
    let file_metadata = tokio::fs::metadata(&file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?; // Error -> 500
    let total_size = file_metadata.len() as f64;
    let bytes_sent = 0u64;

    // Async Stream
    // Create sream form state machine
    // 4KB buffer initial
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
    Response::builder()
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(Body::from_stream(stream))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
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

pub async fn upload(
    Path((token, file_index)): Path<(String, usize)>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Response, StatusCode> {
    // Validate token
    let dest_dir = state
        .sessions
        .validate_and_mark_used(&token)
        .await
        .ok_or(StatusCode::FORBIDDEN)?;

    let mut filename = String::new();
    let mut relative_path = String::new();
    let mut file_data = Vec::new();
    let mut total_files = 1;

    // Parse multipart fields
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        match field.name() {
            Some("file") => {
                filename = field
                    .file_name()
                    .unwrap_or(&format!("file_{}", file_index))
                    .to_string();
                file_data = field
                    .bytes()
                    .await
                    .map_err(|_| StatusCode::BAD_REQUEST)?
                    .to_vec();
            }
            Some("relativePath") => {
                relative_path = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?;
            }
            Some("totalFiles") => {
                let text = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                total_files = text.parse::<usize>().unwrap_or(1);
            }
            _ => {}
        }
    }

    // Decrypt framed data
    let mut decryptor = state.encryptor.create_stream_decryptor();
    let mut buffer = file_data.as_slice();
    let mut plaintext = Vec::new();

    // Parse and decrypt frames
    while buffer.len() >= 4 {
        let len = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;

        if buffer.len() < 4 + len {
            break;
        }

        let encrypted_chunk = &buffer[4..4 + len];
        let decrypted = decryptor.decrypt_next(encrypted_chunk).map_err(|e| {
            eprintln!("Decryption failed: {:?}", e);
            StatusCode::BAD_REQUEST
        })?;

        plaintext.extend_from_slice(&decrypted);
        buffer = &buffer[4 + len..];
    }

    // Determine file path (preserve directory structure)
    let file_path = if !relative_path.is_empty() {
        std::path::Path::new(&dest_dir).join(&relative_path)
    } else {
        std::path::Path::new(&dest_dir).join(&filename)
    };

    // Create parent directories if needed
    if let Some(parent) = file_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            eprintln!("Failed to create directory: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    // Write file
    tokio::fs::write(&file_path, plaintext).await.map_err(|e| {
        eprintln!("Failed to write file: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    println!(
        "Saved file {}/{}: {}",
        file_index + 1,
        total_files,
        file_path.display()
    );

    let response_json = r#"{"success":true}"#;
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(response_json))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

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

pub async fn serve_crypto_js() -> Response {
    const JS: &str = include_str!("../../templates/shared/crypto.js");
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .body(Body::from(JS))
        .unwrap()
}
