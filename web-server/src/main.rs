use std::io;
use std::net::SocketAddr;
use axum::{
    body::{Body, Bytes}, 
    http::StatusCode, 
    response::{IntoResponse, Response}, 
    routing::post, 
    Router
};
use base64::prelude::*;
use tokio::net::TcpListener;
use tracing::info;

// Use 0.0.0.0 so Docker/VPS can expose it to the internet.
// 127.0.0.1 only works if you are on the same machine.
const BIND_ADDR: &str = "0.0.0.0:3000";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("Decompilation task failed: {0}")]
    Join(#[from] tokio::task::JoinError), // Handle thread join errors
}

impl Error {
    fn status_code(&self) -> StatusCode {
        match self {
            Error::Base64(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.status_code(), self.to_string()).into_response()
    }
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    // Setup logging
    tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false)
        .init(); // .init() is a shortcut for set_global_default

    let app = Router::new().route("/decompile", post(decompile));

    let addr: SocketAddr = BIND_ADDR.parse().expect("Invalid address");
    let listener = TcpListener::bind(addr).await?;
    info!("🚀 Listening on {}", listener.local_addr()?);
    
    axum::serve(listener, app).await
}

async fn decompile(body: Bytes) -> Result<String, Error> {
    // 1. Decode Base64 (Fast enough to keep on main thread, but could be moved)
    let mut bytecode = Vec::new();
    BASE64_STANDARD.decode_vec(&body, &mut bytecode)?;

    // 2. CRITICAL FIX: Move heavy CPU work to a blocking thread.
    // This prevents the web server from freezing while decompiling.
    let decompiled = tokio::task::spawn_blocking(move || {
        luau_lifter::decompile_bytecode(&bytecode, 203)
    }).await?; // .await? handles the JoinError

    info!("Successfully decompiled bytecode.");
    Ok(decompiled)
}