use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::Result;
use ferrite::config::Config;
use ferrite::rpc::{JsonRpcRequest, JsonRpcResponse, RpcHandler, StreamChunkSink};
use ferrite::tools::ToolEventSink;
use tokio::sync::Mutex as AsyncMutex;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(io::stderr)
        .init();

    tracing::info!("AI Agent sidecar starting");

    let config = Config::load_default()?;
    let stdin = io::stdin();
    // Single shared stdout handle for streaming notifications, tool events,
    // and final responses.  One mutex for everything prevents output lines
    // from interleaving (a streamChunk line mixed into a JSON-RPC response).
    let notification_stdout: Arc<StdMutex<io::Stdout>> = Arc::new(StdMutex::new(io::stdout()));

    let stream_chunk_sink: StreamChunkSink = {
        let shared_stdout = Arc::clone(&notification_stdout);
        Arc::new(move |chunk| {
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "streamChunk",
                "params": chunk,
            });
            if let Ok(message) = serde_json::to_string(&notification) {
                if let Ok(mut output) = shared_stdout.lock() {
                    let _ = writeln!(output, "{}", message);
                    let _ = output.flush();
                }
            }
        })
    };

    let tool_event_sink: ToolEventSink = {
        let shared_stdout = Arc::clone(&notification_stdout);
        Arc::new(move |event| {
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "toolEvent",
                "params": event,
            });
            match serde_json::to_string(&notification) {
                Ok(message) => {
                    if let Ok(mut output) = shared_stdout.lock() {
                        let _ = writeln!(output, "{}", message);
                        let _ = output.flush();
                    }
                }
                Err(error) => tracing::warn!("Failed to serialize tool event: {}", error),
            }
        })
    };
    let handler = Arc::new(AsyncMutex::new(RpcHandler::with_stream_sink(
        config,
        tool_event_sink,
        stream_chunk_sink,
    )?));

    let mut reader = stdin.lock();
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            tracing::info!("stdin closed, shutting down");
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let resp = match serde_json::from_str::<JsonRpcRequest>(line) {
            Ok(req) => {
                let mut h = handler.lock().await;
                h.handle_request(req).await
            }
            Err(e) => {
                JsonRpcResponse::error(
                    None,
                    -32700,
                    format!("Parse error: {}", e),
                )
            }
        };

        let resp_str = serde_json::to_string(&resp)?;
        if let Ok(mut out) = notification_stdout.lock() {
            let _ = writeln!(out, "{}", resp_str);
            let _ = out.flush();
        }
    }

    Ok(())
}
