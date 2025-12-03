use crate::ui::output;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};
use tracing::{debug, info, warn};

const TUNNEL_URL_TIMEOUT: Duration = Duration::from_secs(15);
const TUNNEL_POLL_INTERVAL: Duration = Duration::from_millis(200);

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
                if let Err(kill_err) = child.kill().await {
                    eprintln!("Failed to kill tunnel process: {}", kill_err);
                }
                return Err(e).context("Failed to obtain tunnel URL");
            }
        };

        output::spinner_success(&spinner, "Tunnel established");

        Ok(Self {
            process: child,
            url,
        })
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        // send kill signal
        if let Err(e) = self.process.kill().await {
            // failed kill often means the process is already dead
            warn!("Failed to send graceful signal to tunnel process: {}", e);
            return Ok(());
        }

        match tokio::time::timeout(Duration::from_secs(5), self.process.wait()).await {
            Ok(Ok(status)) => {
                info!("Tunnel process exited with status: {}", status);
                Ok(())
            }
            Ok(Err(e)) => Err(e).context("Failed to wait for tunnel process"),
            Err(_) => {
                warn!("Tunnel process did not exit after 5 seconds, may be stuck");
                // exhausted attempts, just log
                Ok(())
            }
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

async fn wait_for_url(metrics_port: u16) -> Result<String> {
    let client = reqwest::Client::new();
    let api_url = format!("http://localhost:{}/quicktunnel", metrics_port);

    tokio::time::timeout(TUNNEL_URL_TIMEOUT, async {
        let mut interval = tokio::time::interval(TUNNEL_POLL_INTERVAL);

        loop {
            interval.tick().await;

            match client.get(&api_url).send().await {
                Ok(res) => {
                    if res.status().is_success() {
                        match res.json::<QuickTunnelResponse>().await {
                            Ok(json) if !json.hostname.is_empty() => {
                                return Ok(format!("https://{}", json.hostname));
                            }
                            Ok(_) => {
                                debug!("Waiting for hostname from tunnel...");
                            }
                            Err(e) => {
                                warn!("Failed to parse tunnel response: {}", e);
                            }
                        }
                    } else {
                        debug!("Tunnel metrics returned status: {}", res.status());
                    }
                }
                Err(e) => {
                    debug!("Tunnel not ready yet: {}", e);
                }
            }
        }
    })
    .await
    .context("Timed out waiting for tunnel URL")?
}

fn get_available_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}
