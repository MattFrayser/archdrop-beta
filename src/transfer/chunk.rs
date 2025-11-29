use crate::server::state::AppState;
use crate::transfer::{
    manifest::Manifest,
    util::{hash_path, AppError},
};
use axum::extract::{Multipart, Path, State};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub relative_path: String,
    pub file_name: String,
    pub total_chunks: usize,
    pub file_size: u64,
    pub completed_chunks: HashSet<usize>,
    pub nonce: String,
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

pub async fn parse_chunk_upload(mut multipart: Multipart) -> anyhow::Result<ChunkUpload> {
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
pub async fn load_or_create_metadata(
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

pub async fn save_encrypted_chunk(
    token: &str,
    file_id: &str,
    chunk_index: usize,
    encrypted_data: &[u8],
) -> anyhow::Result<()> {
    let chunk_path = format!("/tmp/archdrop/{}/{}/{}.chunk", token, file_id, chunk_index);
    tokio::fs::write(&chunk_path, encrypted_data).await?;
    Ok(())
}
pub async fn update_chunk_metadata(
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
    State(state): State<AppState>,
) -> Result<axum::Json<Manifest>, AppError> {
    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid or expired token").into());
    }

    let manifest = state
        .session
        .get_manifest()
        .ok_or_else(|| anyhow::anyhow!("No manifest for this session"))?
        .clone();

    Ok(axum::Json(manifest))
}
