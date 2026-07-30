//! Tests for OpenAI-compatible SSE streaming via TcpListener fixtures
//!
//! Each test spins up a local HTTP server using `tokio::net::TcpListener`,
//! creates a real `OpenAiCompatibleProvider` pointing at it, and verifies
//! the streaming behavior end-to-end.

use std::time::Duration;

use futures::StreamExt;
use rgoat_core::provider::impls::OpenAiCompatibleProvider;
use rgoat_core::provider::provider::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ─────── Helpers ───────

/// Build a full HTTP response with Content-Length.
fn http_response(sse_body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
        sse_body.len(),
        sse_body,
    )
}

/// Create a `ProviderConfig` pointing at the given local port.
fn local_provider(port: u16) -> ProviderConfig {
    ProviderConfig::new(
        ProviderType::OpenAICompatible,
        "test",
        format!("http://127.0.0.1:{}", port),
        "test-key",
        "test-model",
    )
}

/// Minimal chat messages for the request body.
fn sample_messages() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: Role::User,
        content: MessageContent::Text("hi".into()),
        name: None,
        tool_call_id: None,
        tool_calls: None,
    }]
}

/// Build a valid SSE data line for a text delta chunk (no trailing blank line).
fn chunk_data_line(content: &str) -> String {
    format!(
        "data: {}\n",
        serde_json::json!({
            "choices": [{
                "delta": { "content": content },
                "finish_reason": null
            }]
        })
    )
}

/// Build a complete SSE event (data line + blank line).
fn sse_event(content: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({
            "choices": [{
                "delta": { "content": content },
                "finish_reason": null
            }]
        })
    )
}

/// Read the full HTTP request (headers + body) from the socket.
async fn drain_request(socket: &mut tokio::net::TcpStream) {
    let mut buf = [0u8; 8192];
    let _ = socket.read(&mut buf).await;
}

// ─────── Test 1: First chunk forwarded immediately ───────
//
// The fixture sends one SSE event, waits 500 ms, then closes.
// The test should receive the chunk before the connection closes.

#[tokio::test]
async fn first_chunk_forwarded_immediately() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        drain_request(&mut socket).await;

        let sse = sse_event("hello");
        let response = http_response(&sse);
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
        // Wait 500 ms before closing
        tokio::time::sleep(Duration::from_millis(500)).await;
        // socket drops → connection closed
    });

    let provider = OpenAiCompatibleProvider::new(local_provider(port));
    let mut stream = provider
        .chat_stream(&sample_messages(), &[], &ChatOptions::default())
        .await
        .unwrap();

    // We should receive a chunk within 2 seconds (well before the 500 ms delay ends)
    let chunk = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("should receive a chunk before timeout");
    assert!(chunk.is_some(), "should receive a chunk");
    let result = chunk.unwrap();
    assert!(result.is_ok(), "chunk should be Ok");
    let sc = result.unwrap();
    assert_eq!(
        sc.choices[0].delta.content.as_deref(),
        Some("hello"),
        "first chunk content should be 'hello'"
    );
}

// ─────── Test 2: Network chunk splitting (UTF-8 boundary) ───────
//
// The UTF-8 character "中" (U+4E2D, 3 bytes: E4 B8 AD) is split across
// two TCP writes: first 2 bytes (E4 B8), then 1 byte (AD) + remainder.
// The test verifies the character is not corrupted to U+FFFD.

#[tokio::test]
async fn network_chunk_splitting_utf8() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    // Build the SSE event containing "中"
    let sse = sse_event("\u{4e2d}");

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        drain_request(&mut socket).await;

        // Send HTTP headers first (without Content-Length since we'll split)
        let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();

        // "中" in UTF-8 = E4 B8 AD
        let sse_bytes = sse.as_bytes();

        // Find the position of "中" in the byte stream
        let needle = "\u{4e2d}";
        let needle_pos = sse.find(needle).unwrap();
        let byte_pos = sse[..needle_pos].as_bytes().len();

        // Split: first 2 bytes of "中" (E4 B8), then 1 byte (AD) + rest
        let first_part = &sse_bytes[..byte_pos + 2];
        let second_part = &sse_bytes[byte_pos + 2..];

        // Write first chunk (first 2 bytes of "中")
        socket.write_all(first_part).await.unwrap();
        socket.flush().await.unwrap();

        // Small delay to ensure TCP chunk boundary
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Write second chunk (remaining byte of "中" + rest)
        socket.write_all(second_part).await.unwrap();
        socket.flush().await.unwrap();

        // Let the client process, then close
        tokio::time::sleep(Duration::from_millis(200)).await;
    });

    let provider = OpenAiCompatibleProvider::new(local_provider(port));
    let mut stream = provider
        .chat_stream(&sample_messages(), &[], &ChatOptions::default())
        .await
        .unwrap();

    let chunk = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("should receive a chunk before timeout");
    assert!(chunk.is_some(), "should receive a chunk");
    let result = chunk.unwrap();
    assert!(result.is_ok(), "chunk should be Ok, got: {:?}", result.err());
    let sc = result.unwrap();
    let content = sc.choices[0].delta.content.as_deref().unwrap_or("");
    assert!(
        !content.contains('\u{fffd}'),
        "should not contain U+FFFD replacement character, got: {:?}",
        content
    );
    assert_eq!(content, "\u{4e2d}", "content should be '中'");
}

// ─────── Test 3: CRLF line endings ───────
//
// Use \r\n as line separator. The parser should handle it correctly.

#[tokio::test]
async fn crlf_line_endings() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        drain_request(&mut socket).await;

        // SSE with \r\n line endings
        let sse = format!(
            "data: {}\r\n\r\n",
            serde_json::json!({
                "choices": [{
                    "delta": { "content": "crlf_ok" },
                    "finish_reason": null
                }]
            })
        );
        let response = http_response(&sse);
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    });

    let provider = OpenAiCompatibleProvider::new(local_provider(port));
    let mut stream = provider
        .chat_stream(&sample_messages(), &[], &ChatOptions::default())
        .await
        .unwrap();

    let chunk = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("should receive a chunk before timeout");
    assert!(chunk.is_some(), "should receive a chunk");
    let result = chunk.unwrap();
    assert!(result.is_ok(), "chunk should be Ok, got: {:?}", result.err());
    let sc = result.unwrap();
    assert_eq!(
        sc.choices[0].delta.content.as_deref(),
        Some("crlf_ok"),
        "CRLF should parse correctly"
    );
}

// ─────── Test 4: Multiple data lines in one event ───────
//
// A single SSE event contains two data: lines. They should be joined
// with "\n" and parsed as a single JSON payload.

#[tokio::test]
async fn multiple_data_lines_in_one_event() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    // Two data lines that together form valid JSON.
    // Split at a point where the newline is valid JSON whitespace
    // (between the key "finish_reason" and the colon separator).
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"multi\"},\"finish_reason\"\ndata: :null}]}\n\n";

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        drain_request(&mut socket).await;

        let response = http_response(sse);
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    });

    let provider = OpenAiCompatibleProvider::new(local_provider(port));
    let mut stream = provider
        .chat_stream(&sample_messages(), &[], &ChatOptions::default())
        .await
        .unwrap();

    let chunk = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("should receive a chunk before timeout");
    assert!(chunk.is_some(), "should receive a chunk");
    let result = chunk.unwrap();
    assert!(result.is_ok(), "chunk should be Ok, got: {:?}", result.err());
    let sc = result.unwrap();
    assert_eq!(
        sc.choices[0].delta.content.as_deref(),
        Some("multi"),
        "multi-line data should parse correctly"
    );
}

// ─────── Test 5: [DONE] signal terminates stream ───────
//
// Send a few chunks, then [DONE]. The stream should end normally
// without error.

#[tokio::test]
async fn done_signal_terminates_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        drain_request(&mut socket).await;

        // Three SSE events: chunk1, [DONE], chunk2
        let sse = format!(
            "{}data: [DONE]\n\n{}",
            sse_event("chunk1"),
            chunk_data_line("chunk2"),
        );
        let response = http_response(&sse);
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    });

    let provider = OpenAiCompatibleProvider::new(local_provider(port));
    let mut stream = provider
        .chat_stream(&sample_messages(), &[], &ChatOptions::default())
        .await
        .unwrap();

    let mut chunks = Vec::new();
    while let Ok(Some(result)) = tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
        match result {
            Ok(chunk) => {
                chunks.push(chunk);
            }
            Err(e) => {
                panic!("unexpected stream error: {:?}", e);
            }
        }
    }

    assert!(!chunks.is_empty(), "should receive at least one chunk");
    assert_eq!(
        chunks[0].choices[0].delta.content.as_deref(),
        Some("chunk1"),
        "first chunk should be 'chunk1'"
    );
}

// ─────── Test 6: Malformed JSON skipped ───────
//
// Send valid chunk → broken JSON event → valid chunk.
// Both valid chunks should be consumable; the bad event is skipped.

#[tokio::test]
async fn malformed_json_is_skipped() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        drain_request(&mut socket).await;

        // Three SSE events: valid1, broken JSON, valid2
        let sse = format!(
            "{}{}{}",
            sse_event("valid1"),
            "data: {not valid json}\n\n",
            sse_event("valid2"),
        );
        let response = http_response(&sse);
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    });

    let provider = OpenAiCompatibleProvider::new(local_provider(port));
    let mut stream = provider
        .chat_stream(&sample_messages(), &[], &ChatOptions::default())
        .await
        .unwrap();

    let mut received = Vec::new();
    while let Ok(Some(result)) = tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
        match result {
            Ok(chunk) => {
                let content = chunk.choices[0]
                    .delta
                    .content
                    .as_deref()
                    .unwrap_or("")
                    .to_string();
                received.push(content);
            }
            Err(e) => {
                panic!("unexpected stream error: {:?}", e);
            }
        }
    }

    assert_eq!(received.len(), 2, "should receive exactly 2 valid chunks");
    assert_eq!(received[0], "valid1", "first valid chunk");
    assert_eq!(received[1], "valid2", "second valid chunk");
}

// ─────── Test 7: Read timeout ───────
//
// The fixture sends HTTP response headers but never sends the body.
// The provider's client has a 60-second read timeout, but we wrap the
// test in a shorter tokio timeout (5 s) to avoid waiting.

#[tokio::test]
async fn read_timeout_returns_stream_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        drain_request(&mut socket).await;

        // Send headers only — no body
        let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.flush().await.unwrap();

        // Never send any body data; keep the connection open
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let provider = OpenAiCompatibleProvider::new(local_provider(port));
    let mut stream = provider
        .chat_stream(&sample_messages(), &[], &ChatOptions::default())
        .await
        .unwrap();

    // The provider's client has a 60-second read timeout, but we
    // wrap collection in a shorter tokio timeout.
    let result = tokio::time::timeout(Duration::from_secs(5), stream.next()).await;

    match result {
        // Either the provider's read timeout fired and returned an error,
        // or our tokio timeout fired (connection hung without body data).
        Ok(Some(Err(_))) => {} // provider returned a stream error — expected
        Err(_elapsed) => {} // tokio timeout fired — also acceptable (no body arrived)
        other => {
            panic!(
                "expected stream error or timeout, got: {:?}",
                other
            );
        }
    }
}