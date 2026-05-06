use axum::{
    Router,
    body::{Body, Bytes},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::prelude::*;
use std::io;
use std::net::SocketAddr;
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

    let app = Router::new()
        .route("/", get(index))
        .route("/decompile", post(decompile));

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
    let decompiled =
        tokio::task::spawn_blocking(move || luau_lifter::decompile_bytecode(&bytecode, 203))
            .await?; // .await? handles the JoinError

    info!("Successfully decompiled bytecode.");
    Ok(decompiled)
}

async fn index() -> impl IntoResponse {
    const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Medal Luau Decompiler</title>
  <style>
    :root { color-scheme: dark; }
    body {
      margin: 0;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif;
      background: #0a0a0b;
      color: #f8f8f8;
      min-height: 100vh;
      display: grid;
      place-items: center;
    }
    .card {
      width: min(1100px, 92vw);
      background: #141417;
      border: 1px solid #2b2b30;
      border-radius: 14px;
      padding: 20px;
      box-shadow: 0 10px 30px rgba(0,0,0,.35);
    }
    h1 { margin: 0 0 8px; font-size: 1.35rem; }
    p { margin: 0 0 16px; color: #bdbdc7; }
    .grid { display: grid; gap: 14px; grid-template-columns: 1fr 1fr; }
    textarea {
      width: 100%;
      min-height: 360px;
      resize: vertical;
      border-radius: 10px;
      border: 1px solid #33333a;
      background: #0f0f12;
      color: #f8f8f8;
      font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
      font-size: 13px;
      line-height: 1.45;
      padding: 12px;
      box-sizing: border-box;
    }
    .actions { margin: 14px 0; display: flex; gap: 10px; align-items: center; }
    button {
      border: 0;
      background: #4f7cff;
      color: #fff;
      border-radius: 9px;
      padding: 10px 14px;
      font-weight: 600;
      cursor: pointer;
    }
    button:disabled { opacity: .7; cursor: wait; }
    #status { color: #bdbdc7; font-size: 0.92rem; }
    @media (max-width: 900px) { .grid { grid-template-columns: 1fr; } }
  </style>
</head>
<body>
  <main class="card">
    <h1>Medal Luau Decompiler</h1>
    <p>Paste base64-encoded Luau bytecode on the left, then decompile it.</p>
    <div class="grid">
      <textarea id="input" spellcheck="false" placeholder="Base64 bytecode..."></textarea>
      <textarea id="output" spellcheck="false" placeholder="Decompiled Luau source..." readonly></textarea>
    </div>
    <div class="actions">
      <button id="run">Decompile</button>
      <span id="status">Ready</span>
    </div>
  </main>
  <script>
    const input = document.getElementById("input");
    const output = document.getElementById("output");
    const run = document.getElementById("run");
    const status = document.getElementById("status");

    async function decompile() {
      const body = input.value.trim();
      if (!body) {
        status.textContent = "Paste base64 bytecode first.";
        return;
      }
      run.disabled = true;
      status.textContent = "Decompiling...";
      try {
        const res = await fetch("/decompile", { method: "POST", body });
        const text = await res.text();
        if (!res.ok) {
          status.textContent = `Failed (${res.status})`;
          output.value = text;
          return;
        }
        output.value = text;
        status.textContent = "Done";
      } catch (_) {
        status.textContent = "Request failed";
      } finally {
        run.disabled = false;
      }
    }

    run.addEventListener("click", decompile);
  </script>
</body>
</html>"#;

    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        Body::from(PAGE),
    )
}
