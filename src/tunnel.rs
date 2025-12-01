use crate::ui::output;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::sleep;

#[derive(Deserialize)]
struct QuickTunnelResponse {
    hostname: String,
}

pub struct CloudflareTunnel {
    process: Child,
    url: String,
}

impl CloudflareTunnel {
    pub async fn start(local_port: u16) -> Result<Self> {
        let spinner = output::spinner("Starting Cloudflare tunnel...");
        spinner.enable_steady_tick(Duration::from_millis(80));

        let metrics_port = get_available_port()
            .ok_or_else(|| anyhow::anyhow!("No free ports for tunnel metrics"))?;

        // spawn cloudflared process & capture output
        let mut child = Command::new("cloudflared")
            .args(&[
                "tunnel",
                "--url",
                &format!("http://localhost:{}", local_port),
                "--metrics",
                &format!("localhost:{}", metrics_port),
                "--no-autoupdate",
                "--protocol",
                "http2",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn cloudflared process")?;

        // Parse stream with timeout
        // reader keeps stream alive after url
        let url = match wait_for_url(metrics_port).await {
            Ok(u) => u,
            Err(e) => {
                let _ = child.start_kill();
                bail!("Failed to obtain tunnel URL: {}", e);
            }
        };

        output::spinner_success(&spinner, "Tunnel established");

        Ok(Self {
            process: child,
            url,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

async fn wait_for_url(metrics_port: u16) -> Result<String> {
    let client = reqwest::Client::new();
    let api_url = format!("http://localhost:{}/quicktunnel", metrics_port);

    for _ in 0..60 {
        match client.get(&api_url).send().await {
            Ok(res) => {
                if res.status().is_success() {
                    let json: QuickTunnelResponse = res.json().await?;
                    if !json.hostname.is_empty() {
                        return Ok(format!("https://{}", json.hostname));
                    }
                }
            }
            Err(_) => {
                // retry
            }
        }
        sleep(Duration::from_millis(200)).await;
    }

    bail!("Timed out waiting for tunnel metrics")
}

fn get_available_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

impl Drop for CloudflareTunnel {
    fn drop(&mut self) {
        let _ = self.process.start_kill();
    }
}
