use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::Duration,
};

use crate::{engine::FraudEngine, index::Decision};

const MAX_REQUEST_BYTES: usize = 1_048_576;

pub fn serve(addr: &str, engine: Arc<FraudEngine>) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let engine = Arc::clone(&engine);
                thread::spawn(move || {
                    let _ = handle_stream(stream, &engine);
                });
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn handle_stream(mut stream: TcpStream, engine: &FraudEngine) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    while let Some(request) = read_http_request(&mut stream)? {
        let close_after_response = request_has_connection_close(&request);
        let response = handle_http_request(&request, engine);
        stream.write_all(&response)?;
        if close_after_response {
            break;
        }
    }
    Ok(())
}

pub fn handle_http_request(request: &[u8], engine: &FraudEngine) -> Vec<u8> {
    let Some(header_end) = find_header_end(request) else {
        return status_response("400 Bad Request", b"{}");
    };
    let header = &request[..header_end];
    let body = &request[header_end + 4..];
    let Ok(header_text) = std::str::from_utf8(header) else {
        return status_response("400 Bad Request", b"{}");
    };
    let mut lines = header_text.lines();
    let Some(request_line) = lines.next() else {
        return status_response("400 Bad Request", b"{}");
    };
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();

    match (method, path) {
        ("GET", "/ready") => status_response("200 OK", b"{}"),
        ("POST", "/fraud-score") => {
            let decision = engine
                .score_bytes(body)
                .unwrap_or_else(|_| Decision::safe_fallback());
            decision_response(decision)
        }
        _ => status_response("404 Not Found", b"{}"),
    }
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Option<Vec<u8>>> {
    let mut buffer = Vec::with_capacity(8192);
    let mut scratch = [0_u8; 4096];
    let mut expected_len = None;

    loop {
        let read = stream.read(&mut scratch)?;
        if read == 0 {
            if buffer.is_empty() {
                return Ok(None);
            }
            break;
        }
        buffer.extend_from_slice(&scratch[..read]);
        if buffer.len() > MAX_REQUEST_BYTES {
            break;
        }
        if expected_len.is_none() {
            if let Some(header_end) = find_header_end(&buffer) {
                let header = &buffer[..header_end];
                let content_length = parse_content_length(header).unwrap_or(0);
                expected_len = Some(header_end + 4 + content_length);
            }
        }
        if let Some(expected_len) = expected_len {
            if buffer.len() >= expected_len {
                break;
            }
        }
    }

    Ok(Some(buffer))
}

fn parse_content_length(header: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(header).ok()?;
    for line in text.lines().skip(1) {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn request_has_connection_close(request: &[u8]) -> bool {
    let Some(header_end) = find_header_end(request) else {
        return true;
    };
    let Ok(header_text) = std::str::from_utf8(&request[..header_end]) else {
        return true;
    };
    header_text.lines().skip(1).any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("connection") && value.trim().eq_ignore_ascii_case("close")
    })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn decision_response(decision: Decision) -> Vec<u8> {
    let body: &'static [u8] = match decision.fraud_count.min(5) {
        0 => b"{\"approved\":true,\"fraud_score\":0.0}",
        1 => b"{\"approved\":true,\"fraud_score\":0.2}",
        2 => b"{\"approved\":true,\"fraud_score\":0.4}",
        3 => b"{\"approved\":false,\"fraud_score\":0.6}",
        4 => b"{\"approved\":false,\"fraud_score\":0.8}",
        _ => b"{\"approved\":false,\"fraud_score\":1.0}",
    };
    status_response("200 OK", body)
}

fn status_response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut response = Vec::with_capacity(128 + body.len());
    response.extend_from_slice(b"HTTP/1.1 ");
    response.extend_from_slice(status.as_bytes());
    response.extend_from_slice(b"\r\nContent-Type: application/json\r\nContent-Length: ");
    response.extend_from_slice(body.len().to_string().as_bytes());
    response.extend_from_slice(b"\r\nConnection: keep-alive\r\n\r\n");
    response.extend_from_slice(body);
    response
}
