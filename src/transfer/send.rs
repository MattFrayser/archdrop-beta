use crate::crypto::{encrypt::Encryptor, stream::EncryptedFileStream, EncryptionKey, Nonce};
use crate::server::state::AppState;
use crate::transfer::util::AppError;
use axum::{
    body::Body,
    extract::{Path, State},
    http::Response,
};
use futures::stream;
use tokio::fs::File;

pub async fn send_handler(
    Path((token, file_index)): Path<(String, usize)>,
    State(state): State<AppState>,
) -> Result<Response<Body>, AppError> {
    // validate token and get file path
    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    let file_entry = state
        .session
        .get_file(file_index)
        .ok_or_else(|| anyhow::anyhow!("invalid file index"))?;

    let session_key = EncryptionKey::from_base64(state.session.session_key())?;
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
