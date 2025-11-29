use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub index: usize,
    pub name: String,
    pub size: u64,
    pub relative_path: String,
    pub nonce: String,

    // server side only
    #[serde(skip)]
    pub full_path: PathBuf,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub files: Vec<FileEntry>,
}

impl Manifest {
    pub fn new(file_paths: Vec<PathBuf>, base_path: Option<&Path>) -> Result<Self> {
        let mut files = Vec::new();

        // determine common base, no base, use parent
        let base =
            base_path.unwrap_or_else(|| file_paths[0].parent().unwrap_or_else(|| Path::new("")));

        for (index, path) in file_paths.iter().enumerate() {
            let metadata = std::fs::metadata(path)?;

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
            });
        }

        Ok(Manifest { files })
    }
}
