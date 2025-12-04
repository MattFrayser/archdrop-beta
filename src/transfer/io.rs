use anyhow::{Context, Result};
use std::fs::File;
use std::sync::Arc;

// Implement required traits based on OS
#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(windows)]
use std::os::windows::fs::FileExt;

pub fn read_chunk_at_position(file_handle: &Arc<File>, start: u64, len: usize) -> Result<Vec<u8>> {
    let mut buffer = vec![0u8; len];

    #[cfg(unix)]
    file_handle
        .read_exact_at(&mut buffer, start)
        .context(format!("Failed to read chunk (unix) at offset {}", start))?;

    #[cfg(windows)]
    {
        // On Windows, seek_read is the equivalent of pread
        let bytes_read = file_handle.seek_read(&mut buffer, start).context(format!(
            "Failed to read chunk (windows) at offset {}",
            start
        ))?;
        if bytes_read != len {
            return Err(anyhow::anyhow!("Unexpected end of file during chunk read"));
        }
    }

    Ok(buffer)
}
