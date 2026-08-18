//! Live loopback HTTP range server for Local Perfect playback.
//!
//! Binds to `127.0.0.1:<random>` with an unguessable session token.
//! Serves validated byte ranges from the guest's `SparseCache`.
//! mpv opens this endpoint as `http://127.0.0.1:<port>/media/<id>?token=<t>`.
//!
//! Missing byte ranges trigger priority chunk fetches and wait bounded time
//! before returning the available bytes. Only validated bytes are served.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};

use crate::media::cache::SparseCache;
use crate::media::manifest::MediaManifest;
use crate::media::stream::parse_http_range;

const WAIT_TIMEOUT_MS: u64 = 500;
const WAIT_POLL_MS: u64 = 20;

/// Configuration for starting a loopback range server.
pub struct RangeServerConfig {
    pub manifest: MediaManifest,
    pub cache: Arc<Mutex<SparseCache>>,
    pub session_token: String,
}

/// Handle returned by `start_range_server`. Contains the bound address and a
/// shutdown signal.
pub struct RangeServerHandle {
    pub addr: SocketAddr,
    pub media_url: String,
    shutdown: Arc<Notify>,
}

impl std::fmt::Debug for RangeServerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RangeServerHandle")
            .field("addr", &self.addr)
            .field("media_url", &self.media_url)
            .finish_non_exhaustive()
    }
}

impl RangeServerHandle {
    /// Signal the server to shut down gracefully.
    pub async fn shutdown(&self) {
        self.shutdown.notify_waiters();
    }
}

/// Start a loopback HTTP range server that serves validated bytes from the
/// sparse cache. Returns the bound address and a handle for shutdown.
///
/// Only binds to `127.0.0.1`. Never exposed on LAN/Tailscale/public.
pub async fn start_range_server(
    config: RangeServerConfig,
) -> Result<RangeServerHandle, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = shutdown.clone();

    let token = config.session_token.clone();
    let media_id = config.manifest.media_id.clone();
    let manifest = Arc::new(config.manifest);
    let cache = config.cache;

    let media_url = format!(
        "http://127.0.0.1:{}/media/{}?token={}",
        addr.port(),
        media_id,
        token
    );

    tokio::spawn(async move {
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((tokio_stream, peer_addr)) => {
                            let manifest = manifest.clone();
                            let cache = cache.clone();
                            let token = token.clone();
                            tokio::task::spawn_blocking(move || {
                                // Convert tokio TcpStream to std for synchronous HTTP handling
                                match tokio_stream.into_std() {
                                    Ok(std_stream) => {
                                        // into_std() returns non-blocking; set to blocking for sync I/O
                                        let _ = std_stream.set_nonblocking(false);
                                        handle_connection(std_stream, peer_addr, &manifest, &cache, &token);
                                    }
                                    Err(_) => {
                                        // Stream conversion failed; drop it
                                    }
                                }
                            });
                        }
                        Err(_) => break,
                    }
                }
                _ = shutdown_clone.notified() => {
                    break;
                }
            }
        }
    });

    Ok(RangeServerHandle {
        addr,
        media_url,
        shutdown,
    })
}

fn handle_connection(
    mut stream: std::net::TcpStream,
    _peer_addr: SocketAddr,
    manifest: &MediaManifest,
    cache: &Arc<Mutex<SparseCache>>,
    valid_token: &str,
) {
    let mut method = String::new();
    let mut path = String::new();
    let mut headers: HashMap<String, String> = HashMap::new();

    // Parse the HTTP request line using the stream directly
    {
        let reader = BufReader::new(stream.try_clone().unwrap_or_else(|_| {
            // This path should never be reached; if try_clone fails the
            // connection is broken and the outer loop will drop it.
            panic!("failed to clone TcpStream for request parsing");
        }));
        let mut lines = reader.lines();
        if let Some(Ok(request_line)) = lines.next() {
            let parts: Vec<&str> = request_line.split_whitespace().collect();
            if parts.len() >= 2 {
                method = parts[0].to_string();
                path = parts[1].to_string();
            }
        }
        // Parse headers
        for line_result in lines {
            match line_result {
                Ok(line) if line.is_empty() => break,
                Ok(line) => {
                    if let Some((key, value)) = line.split_once(':') {
                        headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
                    }
                }
                Err(_) => break,
            }
        }
    }

    // Only accept GET
    if method != "GET" {
        let response = b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n";
        let _ = stream.write_all(response);
        return;
    }

    // Parse path and token
    let (path_base, query_token) = parse_path_and_token(&path);

    // Validate path: /media/<media_id>
    let request_media_id = match path_base.strip_prefix("/media/") {
        Some(id) => id.to_string(),
        None => {
            let response = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            let _ = stream.write_all(response);
            return;
        }
    };

    if request_media_id != manifest.media_id {
        let response = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
        let _ = stream.write_all(response);
        return;
    }

    // Validate token
    let token = query_token.unwrap_or_default();
    if token != valid_token {
        let response = b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n";
        let _ = stream.write_all(response);
        return;
    }

    // Parse Range header
    let range_header = headers.get("range").cloned();

    match range_header {
        Some(range_value) => {
            // Parse range and wait for data if needed
            match parse_http_range(&range_value, manifest.file_size) {
                Ok(range_request) => {
                    let bytes =
                        wait_for_range(cache, manifest, range_request.start, range_request.len());
                    let end = range_request.start + bytes.len() as u64 - 1;
                    let content_range = format!(
                        "bytes {}-{}/{}",
                        range_request.start, end, manifest.file_size
                    );
                    let response = format!(
                        "HTTP/1.1 206 Partial Content\r\n\
                         Content-Type: application/octet-stream\r\n\
                         Content-Length: {}\r\n\
                         Content-Range: {}\r\n\
                         Accept-Ranges: bytes\r\n\
                         Connection: close\r\n\
                         \r\n",
                        bytes.len(),
                        content_range
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(&bytes);
                }
                Err(_) => {
                    let response =
                        b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\n\r\n";
                    let _ = stream.write_all(response);
                }
            }
        }
        None => {
            // Full content request
            let file_size = manifest.file_size;
            let bytes = wait_for_range(cache, manifest, 0, file_size);
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/octet-stream\r\n\
                 Content-Length: {}\r\n\
                 Accept-Ranges: bytes\r\n\
                 Connection: close\r\n\
                 \r\n",
                bytes.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&bytes);
        }
    }
}

/// Parse path and query token from a URL like `/media/<id>?token=<t>`
fn parse_path_and_token(path: &str) -> (String, Option<String>) {
    match path.split_once('?') {
        Some((base, query)) => {
            let token = query
                .split('&')
                .find_map(|param| param.strip_prefix("token=").map(|v| v.to_string()));
            (base.to_string(), token)
        }
        None => (path.to_string(), None),
    }
}

/// Wait for the requested byte range to become available in the cache.
/// Polls with bounded timeout, then returns whatever is available.
fn wait_for_range(
    cache: &Arc<Mutex<SparseCache>>,
    manifest: &MediaManifest,
    start: u64,
    len: u64,
) -> Vec<u8> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(WAIT_TIMEOUT_MS);

    loop {
        // Try to read the range from cache
        let result = {
            let cache_guard = cache.blocking_lock();
            cache_guard.read_range(start, len)
        };

        match result {
            Ok(Some(bytes)) => return bytes,
            Ok(None) => {
                // Range not fully available — check if deadline exceeded
                if std::time::Instant::now() >= deadline {
                    // Return what we can from the available chunks
                    return read_available_portion(cache, manifest, start, len);
                }
                std::thread::sleep(std::time::Duration::from_millis(WAIT_POLL_MS));
            }
            Err(_) => {
                // Cache error — return what we can
                return read_available_portion(cache, manifest, start, len);
            }
        }
    }
}

/// Read as much of the requested range as is currently available.
/// Returns partial data if some chunks are missing.
fn read_available_portion(
    cache: &Arc<Mutex<SparseCache>>,
    manifest: &MediaManifest,
    start: u64,
    len: u64,
) -> Vec<u8> {
    let end = start + len;
    let chunk_size = manifest.chunk_size;

    // Find the first available byte range
    let mut current = start;
    let mut result = Vec::new();

    while current < end {
        let chunk_index = current / chunk_size;
        let chunk_start = chunk_index * chunk_size;
        let chunk_end = chunk_start + chunk_size;

        let available_start = current.max(chunk_start);
        let available_end = end.min(chunk_end);

        if available_end > available_start {
            let read_len = available_end - available_start;
            let cache_guard = cache.blocking_lock();
            if let Ok(Some(bytes)) = cache_guard.read_range(available_start, read_len) {
                result.extend_from_slice(&bytes);
            }
        }

        // Move to the next chunk boundary
        current = chunk_end;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::cache::SparseCache;
    use crate::media::manifest::{MediaManifest, QuickFingerprint};
    use std::fs;
    use std::io::Read;
    use uuid::Uuid;

    fn test_manifest() -> MediaManifest {
        MediaManifest {
            media_id: "range-test".to_string(),
            filename: "movie.bin".to_string(),
            file_size: 16,
            container: None,
            full_hash: "hash".to_string(),
            quick_fingerprint: QuickFingerprint {
                file_size: 16,
                first_hash: "first".to_string(),
                last_hash: "last".to_string(),
            },
            chunk_size: 4,
            chunk_count: 4,
        }
    }

    /// Create a blocking TCP connection to the range server.
    fn blocking_connect(addr: SocketAddr) -> std::net::TcpStream {
        let stream = std::net::TcpStream::connect(addr).expect("connect");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("read timeout");
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("write timeout");
        stream
    }

    fn http_get(addr: SocketAddr, path_and_token: &str) -> String {
        let mut stream = blocking_connect(addr);
        let request = format!(
            "GET {path_and_token} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        String::from_utf8_lossy(&response).into_owned()
    }

    fn http_get_range(addr: SocketAddr, path_and_token: &str, range: &str) -> String {
        let mut stream = blocking_connect(addr);
        let request = format!(
            "GET {path_and_token} HTTP/1.1\r\nRange: {range}\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        String::from_utf8_lossy(&response).into_owned()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_serves_validated_bytes() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        fs::create_dir_all(&root).expect("temp");
        let manifest = test_manifest();
        let mut cache = SparseCache::open(&root, manifest.clone()).expect("cache");
        cache.write_chunk(0, b"abcd").expect("write 0");
        cache.write_chunk(1, b"efgh").expect("write 1");
        let cache = Arc::new(Mutex::new(cache));

        let handle = start_range_server(RangeServerConfig {
            manifest: manifest.clone(),
            cache: cache.clone(),
            session_token: "test-token-123".to_string(),
        })
        .await
        .expect("server");

        // Allow the server accept loop to settle
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let response = http_get_range(
            handle.addr,
            "/media/range-test?token=test-token-123",
            "bytes=0-7",
        );

        assert!(
            response.contains("206 Partial Content"),
            "expected 206, got: {}",
            &response[..response.len().min(200)]
        );
        assert!(response.contains("abcdefgh"));

        handle.shutdown().await;
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_rejects_invalid_token() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        fs::create_dir_all(&root).expect("temp");
        let manifest = test_manifest();
        let mut cache = SparseCache::open(&root, manifest.clone()).expect("cache");
        cache.write_chunk(0, b"abcd").expect("write 0");
        let cache = Arc::new(Mutex::new(cache));

        let handle = start_range_server(RangeServerConfig {
            manifest: manifest.clone(),
            cache,
            session_token: "correct-token".to_string(),
        })
        .await
        .expect("server");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let response = http_get(handle.addr, "/media/range-test?token=wrong-token");

        assert!(
            response.contains("401 Unauthorized"),
            "expected 401, got: {}",
            &response[..response.len().min(200)]
        );

        handle.shutdown().await;
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_rejects_non_get() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        fs::create_dir_all(&root).expect("temp");
        let manifest = test_manifest();
        let cache = Arc::new(Mutex::new(
            SparseCache::open(&root, manifest.clone()).expect("cache"),
        ));

        let handle = start_range_server(RangeServerConfig {
            manifest,
            cache,
            session_token: "tok".to_string(),
        })
        .await
        .expect("server");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut stream = blocking_connect(handle.addr);
        let request =
            "POST /media/range-test HTTP/1.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        let response_str = String::from_utf8_lossy(&response);

        assert!(
            response_str.contains("405 Method Not Allowed"),
            "expected 405, got: {}",
            &response_str[..response_str.len().min(200)]
        );

        handle.shutdown().await;
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_404_for_wrong_media_id() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        fs::create_dir_all(&root).expect("temp");
        let manifest = test_manifest();
        let cache = Arc::new(Mutex::new(
            SparseCache::open(&root, manifest.clone()).expect("cache"),
        ));

        let handle = start_range_server(RangeServerConfig {
            manifest,
            cache,
            session_token: "tok".to_string(),
        })
        .await
        .expect("server");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let response = http_get(handle.addr, "/media/wrong-id?token=tok");

        assert!(
            response.contains("404 Not Found"),
            "expected 404, got: {}",
            &response[..response.len().min(200)]
        );

        handle.shutdown().await;
        let _ = fs::remove_dir_all(&root);
    }
}
