use axum::{
    body::Body,
    extract::Path,
    http::{Response, StatusCode},
    response::Html,
};

pub async fn serve_upload_page(Path(token): Path<String>) -> Result<Html<&'static str>, StatusCode> {
    eprintln!("[server] Serving upload page for token: {}", token);
    // return embedded html to brower
    const HTML: &str = include_str!("../../templates/upload/upload.html");
    Ok(Html(HTML))
}

pub async fn serve_upload_js() -> Response<Body> {
    const JS: &str = include_str!("../../templates/upload/upload.js");
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .body(Body::from(JS))
        .unwrap()
}

pub async fn serve_download_page(Path(token): Path<String>) -> Result<Html<&'static str>, StatusCode> {
    eprintln!("[server] Serving download page for token: {}", token);
    // return embedded html to brower
    const HTML: &str = include_str!("../../templates/download/download.html");
    Ok(Html(HTML))
}

pub async fn serve_download_js() -> Response<Body> {
    const JS: &str = include_str!("../../templates/download/download.js");
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .body(Body::from(JS))
        .unwrap()
}

pub async fn serve_crypto_js() -> Response<Body> {
    const JS: &str = include_str!("../../templates/shared/crypto.js");
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .body(Body::from(JS))
        .unwrap()
}
