use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

#[derive(Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub index: usize,
    pub name: String,
    #[serde(skip)]
    pub full_path: PathBuf,
    pub relative_path: String,
    pub size: u64,
    pub nonce: String,
    pub sha256: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub files: Vec<FileEntry>,
}

impl Manifest {
    pub async fn new(file_paths: Vec<PathBuf>, base_path: Option<&Path>) -> Result<Self> {
        let mut files = Vec::new();

        // determine common base, no base, use parent
        let base =
            base_path.unwrap_or_else(|| file_paths[0].parent().unwrap_or_else(|| Path::new("")));

        for (index, path) in file_paths.iter().enumerate() {
            let metadata = std::fs::metadata(path)?;
            let sha256 = calculate_file_hash(&path).await?;
            let relative = path
                .strip_prefix(base)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .to_string();

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unnamed")
                .to_string();

            // Unique nonce for each file
            let nonce = crate::crypto::Nonce::new();

            files.push(FileEntry {
                index,
                name,
                size: metadata.len(),
                relative_path: relative,
                nonce: nonce.to_base64(),
                full_path: path.clone(),
                sha256,
            });
        }

        Ok(Manifest { files })
    }
}

async fn calculate_file_hash(path: &Path) -> Result<String> {
    const CHUNK_SIZE: usize = 64 * 1024;

    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK_SIZE];

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}
