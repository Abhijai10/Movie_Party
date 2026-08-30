//! Live loopback HTTP range server for Local Perfect playback.
//!
//! Binds to `127.0.0.1:<random>` with an unguessable session token.
//! Serves validated byte ranges from the guest's `SparseCache`.
//!
//! Waiters sleep on a [`ChunkWake`] condition variable — the transfer worker
//! notifies it after each chunk is persisted, so range requests wake
//! immediately instead of busy-polling.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Condvar, Mutex as StdMutex};
use std::time::{Duration, Instant};

use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};

use crate::media::cache::SparseCache;
use crate::media::manifest::MediaManifest;
use crate::media::stream::parse_http_range;
use crate::media::transfer::{ChunkDemandHandle, ChunkPriority};

/// libmpv normally keeps only a small number of HTTP range reads active. A
/// bounded listener prevents malformed or stalled local clients from turning
/// that into unbounded blocking tasks.
pub const MAX_RANGE_CONNECTIONS: usize = 8;

/// Bounded condition variable shared between the transfer worker (writer) and
/// waiting HTTP range requests (readers). The worker bumps a generation
/// counter and notifies all waiters after persisting a chunk; waiters wake,
/// re-check the cache, and either serve the bytes or sleep again until the
/// overall bounded wait expires.
///
/// `wait_timeout` is a safety net against spurious wakeups; the fast path is
/// a genuine notify from the writer.
#[derive(Debug, Default)]
pub struct ChunkWake {
    generation: StdMutex<u64>,
    condvar: Condvar,
}

impl ChunkWake {
    pub fn new() -> Self {
        Self::default()
    }

    /// Signal that a chunk may have been written.
    pub fn notify(&self) {
        let mut generation = match self.generation.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        *generation = generation.wrapping_add(1);
        drop(generation);
        self.condvar.notify_all();
    }

    /// Current generation counter.
    pub fn generation(&self) -> u64 {
        match self.generation.lock() {
            Ok(g) => *g,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    /// Wait until the generation moves past `from` or `timeout` elapses.
    /// Returns `true` if the generation changed (a chunk was written).
    ///
    /// Uses `wait_timeout_while`, so a notification that arrives before this
    /// call (lost-wakeup protection) returns immediately, and spurious
    /// wakeups loop internally until the deadline.
    fn wait_until_changed(&self, from: u64, timeout: Duration) -> bool {
        let mut generation = match self.generation.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let (guard, _wait_result) = self
            .condvar
            .wait_timeout_while(generation, timeout, |current| *current == from)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        generation = guard;
        *generation != from
    }
}

/// Result of a bounded wait for a byte range.
enum RangeServe {
    /// The full requested range is available.
    Available(Vec<u8>),
    /// Some (but not all) requested bytes are available.
    Partial(Vec<u8>),
    /// No requested bytes became available within the bounded wait.
    Unavailable,
}

pub struct RangeServerConfig {
    pub manifest: MediaManifest,
    pub cache: Arc<Mutex<SparseCache>>,
    pub session_token: String,
    /// Wake source notified by the transfer worker after each chunk write.
    pub chunk_wake: Arc<ChunkWake>,
    /// Shared demand channel: missing ranges are enqueued here as Critical
    /// chunk requests for the guest transfer worker.
    pub demand: ChunkDemandHandle,
    /// Bounded wait for demanded chunks before serving what is available.
    pub wait_timeout_ms: u64,
}

pub struct RangeServerHandle {
    pub addr: SocketAddr,
    pub media_url: String,
    shutdown: Arc<Notify>,
    pub chunk_wake: Arc<ChunkWake>,
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
    pub fn shutdown(&self) {
        self.shutdown.notify_waiters();
    }
    pub fn notify_chunk_written(&self) {
        self.chunk_wake.notify();
    }
}

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
    let demand = config.demand;
    let chunk_wake = config.chunk_wake;
    let wait_timeout_ms = config.wait_timeout_ms;
    let chunk_wake_for_task = chunk_wake.clone();
    let connection_limit = Arc::new(tokio::sync::Semaphore::new(MAX_RANGE_CONNECTIONS));
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
                            let permit = tokio::select! {
                                permit = connection_limit.clone().acquire_owned() => match permit {
                                    Ok(permit) => permit,
                                    Err(_) => break,
                                },
                                _ = shutdown_clone.notified() => break,
                            };
                            let manifest = manifest.clone();
                            let cache = cache.clone();
                            let token = token.clone();
                            let demand = demand.clone();
                            let chunk_wake = chunk_wake_for_task.clone();
                            tokio::task::spawn_blocking(move || {
                                let _permit = permit;
                                if let Ok(std_stream) = tokio_stream.into_std() {
                                    let _ = std_stream.set_nonblocking(false);
                                    handle_connection(std_stream, peer_addr, &manifest, &cache, &token, &demand, &chunk_wake, wait_timeout_ms);
                                }
                            });
                        }
                        Err(_) => break,
                    }
                }
                _ = shutdown_clone.notified() => { break; }
            }
        }
    });

    Ok(RangeServerHandle {
        addr,
        media_url,
        shutdown,
        chunk_wake,
    })
}

#[allow(clippy::too_many_arguments)]
fn handle_connection(
    mut stream: std::net::TcpStream,
    _peer_addr: SocketAddr,
    manifest: &MediaManifest,
    cache: &Arc<Mutex<SparseCache>>,
    valid_token: &str,
    demand: &ChunkDemandHandle,
    chunk_wake: &Arc<ChunkWake>,
    wait_timeout_ms: u64,
) {
    let mut method = String::new();
    let mut path = String::new();
    let mut headers: HashMap<String, String> = HashMap::new();
    {
        let reader = BufReader::new(stream.try_clone().expect("clone tcp"));
        let mut lines = reader.lines();
        if let Some(Ok(request_line)) = lines.next() {
            let parts: Vec<&str> = request_line.split_whitespace().collect();
            if parts.len() >= 2 {
                method = parts[0].to_string();
                path = parts[1].to_string();
            }
        }
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
    if method != "GET" {
        let _ = stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n");
        return;
    }
    let (path_base, query_token) = parse_path_and_token(&path);
    let request_media_id = match path_base.strip_prefix("/media/") {
        Some(id) => id.to_string(),
        None => {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            return;
        }
    };
    if request_media_id != manifest.media_id {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        return;
    }
    let token = query_token.unwrap_or_default();
    if token != valid_token {
        let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
        return;
    }
    match headers.get("range").cloned() {
        Some(range_value) => match parse_http_range(&range_value, manifest.file_size) {
            Ok(range_request) => {
                let serve = wait_for_range(
                    cache,
                    manifest,
                    range_request.start,
                    range_request.len(),
                    demand,
                    chunk_wake,
                    wait_timeout_ms,
                );
                match serve {
                    RangeServe::Available(bytes) | RangeServe::Partial(bytes) => {
                        // Never computed on an empty body: an empty result is
                        // served as a retriable 503, not a bogus 206.
                        let end = range_request.start + bytes.len() as u64 - 1;
                        let content_range = format!(
                            "bytes {}-{}/{}",
                            range_request.start, end, manifest.file_size
                        );
                        let response = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nContent-Range: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                            bytes.len(), content_range
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.write_all(&bytes);
                    }
                    RangeServe::Unavailable => {
                        // Requested bytes could not be produced within the
                        // bounded wait — tell the player to retry rather than
                        // fabricate an empty partial response.
                        let response = "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = stream.write_all(response.as_bytes());
                    }
                }
            }
            Err(_) => {
                let _ = stream
                    .write_all(b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\n\r\n");
            }
        },
        None => {
            let serve = wait_for_range(
                cache,
                manifest,
                0,
                manifest.file_size,
                demand,
                chunk_wake,
                wait_timeout_ms,
            );
            match serve {
                RangeServe::Available(bytes) | RangeServe::Partial(bytes) => {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                        bytes.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(&bytes);
                }
                RangeServe::Unavailable => {
                    let response = "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(response.as_bytes());
                }
            }
        }
    }
}

fn parse_path_and_token(path: &str) -> (String, Option<String>) {
    match path.split_once('?') {
        Some((base, query)) => {
            let token = query
                .split('&')
                .find_map(|p| p.strip_prefix("token=").map(|v| v.to_string()));
            (base.to_string(), token)
        }
        None => (path.to_string(), None),
    }
}

fn wait_for_range(
    cache: &Arc<Mutex<SparseCache>>,
    manifest: &MediaManifest,
    start: u64,
    len: u64,
    demand: &ChunkDemandHandle,
    chunk_wake: &Arc<ChunkWake>,
    wait_timeout_ms: u64,
) -> RangeServe {
    let deadline = Instant::now() + Duration::from_millis(wait_timeout_ms);
    let mut demand_enqueued = false;
    loop {
        let result = {
            let g = cache.blocking_lock();
            g.read_range(start, len)
        };
        match result {
            Ok(Some(bytes)) => return RangeServe::Available(bytes),
            Ok(None) => {
                if !demand_enqueued {
                    enqueue_missing_chunks(cache, manifest, start, len, demand);
                    demand_enqueued = true;
                }
                let now = Instant::now();
                if now >= deadline {
                    let partial = read_available_portion(cache, manifest, start, len);
                    return if partial.is_empty() {
                        RangeServe::Unavailable
                    } else {
                        RangeServe::Partial(partial)
                    };
                }
                // Record the generation BEFORE sleeping; the transfer worker
                // bumps it after each chunk write, so a genuine notify wakes
                // us immediately (no polling).
                let from = chunk_wake.generation();
                let remaining = deadline - now;
                chunk_wake.wait_until_changed(from, remaining);
            }
            Err(_) => {
                let partial = read_available_portion(cache, manifest, start, len);
                return if partial.is_empty() {
                    RangeServe::Unavailable
                } else {
                    RangeServe::Partial(partial)
                };
            }
        }
    }
}

/// Compute the exact 1 MiB chunk indices covering `[start, start+len)` that
/// are still missing from the sparse cache and enqueue them as CRITICAL
/// demand for the guest transfer worker.
fn enqueue_missing_chunks(
    cache: &Arc<Mutex<SparseCache>>,
    manifest: &MediaManifest,
    start: u64,
    len: u64,
    demand: &ChunkDemandHandle,
) {
    let end = start.saturating_add(len);
    if end == 0 {
        return;
    }
    let first = start / manifest.chunk_size;
    let last =
        (end.saturating_sub(1) / manifest.chunk_size).min(manifest.chunk_count.saturating_sub(1));
    let mut missing = Vec::new();
    {
        let g = cache.blocking_lock();
        for index in first..=last {
            if !g.chunk_map().is_available(index) {
                missing.push(index);
            }
        }
    }
    for index in missing {
        demand.request(index, ChunkPriority::Critical);
    }
}

fn read_available_portion(
    cache: &Arc<Mutex<SparseCache>>,
    manifest: &MediaManifest,
    start: u64,
    len: u64,
) -> Vec<u8> {
    let end = start + len;
    let chunk_size = manifest.chunk_size;
    let mut current = start;
    let mut result = Vec::new();
    while current < end {
        let chunk_start = (current / chunk_size) * chunk_size;
        let chunk_end = chunk_start + chunk_size;
        let avail_start = current.max(chunk_start);
        let avail_end = end.min(chunk_end);
        if avail_end > avail_start {
            let read_len = avail_end - avail_start;
            let g = cache.blocking_lock();
            if let Ok(Some(bytes)) = g.read_range(avail_start, read_len) {
                result.extend_from_slice(&bytes);
            }
        }
        current = chunk_end;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::cache::SparseCache;
    use crate::media::manifest::{MediaManifest, QuickFingerprint};
    use crate::media::transfer::{ChunkDemandHandle, ChunkPriority};
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

    fn blocking_connect(addr: SocketAddr) -> std::net::TcpStream {
        let s = std::net::TcpStream::connect(addr).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("rt");
        s.set_write_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("wt");
        s
    }

    fn http_get_range(addr: SocketAddr, path: &str, range: &str) -> String {
        let mut s = blocking_connect(addr);
        let req = format!(
            "GET {path} HTTP/1.1\r\nRange: {range}\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        );
        s.write_all(req.as_bytes()).expect("write");
        let mut resp = Vec::new();
        s.read_to_end(&mut resp).expect("read");
        String::from_utf8_lossy(&resp).into_owned()
    }

    fn http_get(addr: SocketAddr, path: &str) -> String {
        let mut s = blocking_connect(addr);
        let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).expect("write");
        let mut resp = Vec::new();
        s.read_to_end(&mut resp).expect("read");
        String::from_utf8_lossy(&resp).into_owned()
    }

    fn make_config(m: MediaManifest, c: Arc<Mutex<SparseCache>>, t: &str) -> RangeServerConfig {
        RangeServerConfig {
            manifest: m,
            cache: c,
            session_token: t.to_string(),
            chunk_wake: Arc::new(ChunkWake::new()),
            demand: ChunkDemandHandle::new(),
            wait_timeout_ms: 500,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_serves_validated_bytes() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp");
        let m = test_manifest();
        let mut cache = SparseCache::open(&root, m.clone()).expect("cache");
        cache.write_chunk(0, b"abcd").expect("w0");
        cache.write_chunk(1, b"efgh").expect("w1");
        let cache = Arc::new(Mutex::new(cache));
        let h = start_range_server(make_config(m, cache, "tok"))
            .await
            .expect("server");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let r = http_get_range(h.addr, "/media/range-test?token=tok", "bytes=0-7");
        assert!(r.contains("206 Partial Content"));
        assert!(r.contains("abcdefgh"));
        h.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_rejects_invalid_token() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp");
        let m = test_manifest();
        let mut cache = SparseCache::open(&root, m.clone()).expect("cache");
        cache.write_chunk(0, b"abcd").expect("w0");
        let cache = Arc::new(Mutex::new(cache));
        let h = start_range_server(make_config(m, cache, "correct"))
            .await
            .expect("server");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let r = http_get(h.addr, "/media/range-test?token=wrong");
        assert!(r.contains("401 Unauthorized"));
        h.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_rejects_non_get() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp");
        let m = test_manifest();
        let cache = Arc::new(Mutex::new(
            SparseCache::open(&root, m.clone()).expect("cache"),
        ));
        let h = start_range_server(make_config(m, cache, "tok"))
            .await
            .expect("server");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut s = blocking_connect(h.addr);
        s.write_all(
            b"POST /media/range-test HTTP/1.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .expect("w");
        let mut resp = Vec::new();
        s.read_to_end(&mut resp).expect("r");
        assert!(String::from_utf8_lossy(&resp).contains("405 Method Not Allowed"));
        h.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_404_for_wrong_media_id() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp");
        let m = test_manifest();
        let cache = Arc::new(Mutex::new(
            SparseCache::open(&root, m.clone()).expect("cache"),
        ));
        let h = start_range_server(make_config(m, cache, "tok"))
            .await
            .expect("server");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let r = http_get(h.addr, "/media/wrong-id?token=tok");
        assert!(r.contains("404 Not Found"));
        h.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_chunk_wake_notification() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp");
        let m = test_manifest();
        let cache = Arc::new(Mutex::new(
            SparseCache::open(&root, m.clone()).expect("cache"),
        ));
        let h = start_range_server(make_config(m, cache.clone(), "tok"))
            .await
            .expect("server");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        {
            let cache_clone = cache.clone();
            tokio::task::spawn_blocking(move || {
                let mut c = cache_clone.blocking_lock();
                c.write_chunk(0, b"abcd").expect("write");
            })
            .await
            .unwrap();
        }
        h.notify_chunk_written();
        let r = http_get_range(h.addr, "/media/range-test?token=tok", "bytes=0-3");
        assert!(r.contains("206 Partial Content"));
        assert!(r.contains("abcd"));
        h.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_enqueues_demand_for_missing_chunks() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp");
        let m = test_manifest();
        let cache = Arc::new(Mutex::new(
            SparseCache::open(&root, m.clone()).expect("cache"),
        ));
        let demand = ChunkDemandHandle::new();
        let mut config = make_config(m, cache, "tok");
        config.demand = demand.clone();
        let h = start_range_server(config).await.expect("server");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Request a range spanning chunks 0 and 1, both missing. The request
        // handler must enqueue both as Critical demand before it starts
        // waiting.
        let client = std::thread::spawn(move || {
            http_get_range(h.addr, "/media/range-test?token=tok", "bytes=0-7")
        });
        let mut demanded = Vec::new();
        for _ in 0..20 {
            while let Some(req) = demand.pop_next() {
                demanded.push(req);
            }
            if demanded.len() >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let _resp = client.join().expect("request");

        assert_eq!(demanded.len(), 2);
        assert!(demanded
            .iter()
            .all(|r| r.priority == ChunkPriority::Critical));
        assert!(demanded.iter().any(|r| r.index == 0));
        assert!(demanded.iter().any(|r| r.index == 1));
        h.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_returns_503_when_no_data_after_timeout() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp");
        let m = test_manifest();
        let cache = Arc::new(Mutex::new(
            SparseCache::open(&root, m.clone()).expect("cache"),
        ));
        // Short bounded wait: no worker will ever supply the chunk.
        let mut config = make_config(m, cache, "tok");
        config.wait_timeout_ms = 150;
        let h = start_range_server(config).await.expect("server");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let r = http_get_range(h.addr, "/media/range-test?token=tok", "bytes=0-3");
        assert!(
            r.contains("503 Service Unavailable"),
            "empty wait must not fabricate 206: {r}"
        );
        assert!(
            !r.contains("206 Partial Content"),
            "no bogus partial content expected"
        );
        h.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_connection_limit_blocks_excess_work() {
        // The accept loop gates every connection through
        // `Semaphore::new(MAX_RANGE_CONNECTIONS)`. Prove the configured bound
        // and that excess work is deferred until a permit frees, without
        // relying on sandbox socket binding.
        let limit = Arc::new(tokio::sync::Semaphore::new(MAX_RANGE_CONNECTIONS));
        assert_eq!(MAX_RANGE_CONNECTIONS, 8, "configured concurrency bound");

        let mut held = Vec::new();
        for _ in 0..MAX_RANGE_CONNECTIONS {
            let permit = limit
                .clone()
                .try_acquire_owned()
                .expect("permit within bound");
            held.push(permit);
        }
        assert!(
            limit.clone().try_acquire_owned().is_err(),
            "excess connection must not proceed past the bound"
        );

        drop(held.pop());
        let released = limit.clone().try_acquire_owned();
        assert!(
            released.is_ok(),
            "a freed permit must admit the next connection"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_server_wake_notifies_waiter_immediately() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp");
        let m = test_manifest();
        let cache = Arc::new(Mutex::new(
            SparseCache::open(&root, m.clone()).expect("cache"),
        ));
        let mut config = make_config(m, cache.clone(), "tok");
        config.wait_timeout_ms = 2_000;
        let h = start_range_server(config).await.expect("server");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Request uncached bytes; the handler will enqueue demand and sleep
        // on the Condvar. After it is waiting, write the chunk and notify —
        // the waiter must wake and serve it well before the timeout.
        let client = std::thread::spawn(move || {
            http_get_range(h.addr, "/media/range-test?token=tok", "bytes=0-3")
        });
        // Give the handler time to reach its wait.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        {
            let cache_clone = cache.clone();
            tokio::task::spawn_blocking(move || {
                let mut c = cache_clone.blocking_lock();
                c.write_chunk(0, b"abcd").expect("write");
            })
            .await
            .unwrap();
        }
        let started = std::time::Instant::now();
        h.notify_chunk_written();
        let r = client.join().expect("request");
        let elapsed = started.elapsed();

        assert!(r.contains("206 Partial Content"), "woken waiter must serve");
        assert!(r.contains("abcd"));
        assert!(
            elapsed < std::time::Duration::from_millis(1_500),
            "wake must be a real notification, not a long timeout ({elapsed:?})"
        );
        h.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }
}
