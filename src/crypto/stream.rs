use aes_gcm::{
    Aes256Gcm,
    aead::stream::EncryptorBE32,
};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;

pub struct EncryptedFileStream {
    file: File,
    encryptor: EncryptorBE32<Aes256Gcm>,
    buffer: [u8; 65536],
    bytes_sent: u64,
    total_size: u64,
    progress_sender: watch::Sender<f64>,
}

impl EncryptedFileStream {
    pub fn new(
        file: File,
        encryptor: EncryptorBE32<Aes256Gcm>,
        total_size: u64,
        progress_sender: watch::Sender<f64>,
    ) -> Self {
        Self {
            file,
            encryptor,
            buffer: [0u8; 65536],
            bytes_sent: 0,
            total_size,
            progress_sender,
        }
    }

    pub async fn read_next_chunk(&mut self) -> Option<Result<Vec<u8>, std::io::Error>> {
        match self.file.read(&mut self.buffer).await {
            // EOF
            Ok(0) => {
                let _ = self.progress_sender.send(100.0);
                None
            }
            Ok(n) => {
                let chunk = &self.buffer[..n];

                //  encrypt chunk
                let encrypted = match self.encryptor.encrypt_next(chunk) {
                    Ok(enc) => enc,
                    Err(e) => {
                        return Some(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Encryption failed: {:?}", e),
                        )))
                    }
                };

                // Frame = [4 byte len][encypted data]
                let len = encrypted.len() as u32;
                let mut framed = len.to_be_bytes().to_vec();
                framed.extend_from_slice(&encrypted);

                // update progress
                self.bytes_sent += n as u64;
                let progress = (self.bytes_sent as f64 / self.total_size as f64) * 100.0;
                let _ = self.progress_sender.send(progress);

                Some(Ok(framed))
            }
            Err(e) => Some(Err(e)),
        }
    }
}
