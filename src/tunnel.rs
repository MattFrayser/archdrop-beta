use futures::FutureExt;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::signal;

pub struct CloudflareTunnel {
    process: Child,
    url: String,
}

impl CloudflareTunnel {
    pub async fn start(local_port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        // check cloudflared installed
        if !Self::is_installed().await {
            return Err("cloudflared not installed.\n 
                Install cloudflared or use --local."
                .into());
        }

        println!("cloudflared installed");

        // spawn cloudflared process & capture output
        let mut child = Command::new("cloudflared")
            .args(&[
                "tunnel",
                "--url",
                &format!("http://localhost:{}", local_port),
                "--no-autoupdate",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().ok_or("No stdout")?;
        let stderr = child.stderr.take().ok_or("No stderr")?;

        // Parse both streams with timeout
        let url = tokio::select! {
            result = Self::parse_stream(stdout) => result?,
            result = Self::parse_stream(stderr) => result?,
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                return Err("Tunnel startup timed out after 30 seconds".into());
            }
        };

        println!("{:?}", url);

        Ok(Self {
            process: child,
            url,
        })
    }

    async fn parse_stream(
        stream: impl tokio::io::AsyncRead + Unpin,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let reader = BufReader::new(stream);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            if line.contains("trycloudflare.com") {
                if let Some(url) = Self::extract_url(&line) {
                    return Ok(url);
                }
            }
        }

        Err("No tunnel URL found".into())
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    async fn is_installed() -> bool {
        Command::new("which")
            .arg("cloudflared")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await // ← Add this
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn extract_url(line: &str) -> Option<String> {
        if line.contains("trycloudflare.com") {
            // Find where https:// starts
            if let Some(start) = line.find("https://") {
                let rest = &line[start..];

                // Extract until whitespace or box characters
                let url = rest
                    .split_whitespace()
                    .next()?
                    .trim_end_matches(&[',', '.', ';', '|', '+', '-', ' '][..]);

                // Verify it's a valid URL
                if url.starts_with("https://") && url.contains("trycloudflare.com") {
                    return Some(url.to_string());
                }
            }
        }

        None
    }
}

impl Drop for CloudflareTunnel {
    fn drop(&mut self) {
        let _ = self.process.start_kill();
    }
}
