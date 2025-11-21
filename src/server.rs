use axum::{Router, routing::get};
use std::net::SocketAddr;

pub async fn start_server() -> Result<u16, Box<dyn std::error::Error>> {

    //  port left 0 for OS to choose
    let addr = SocketAddr::from(([0, 0, 0, 0], 0)); // listen on all interfaces
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_port = listener.local_addr()?.port();

    println!("listening on port {}", actual_port);

    // Create axium router
    let app = Router::new()
        .route("/health", get(|| async { "OK" }));

    // Start server
    axum::serve(listener, app).await?;

    Ok(actual_port)
}
