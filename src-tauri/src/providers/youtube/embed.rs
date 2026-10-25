//! A loopback, capability-URL origin for the official IFrame API wrapper.
//! It serves three compiled assets only: no proxy, IPC, filesystem or cookies.
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
pub struct EmbedServer {
    pub origin: String,
    pub page: String,
    stop: Arc<AtomicBool>,
    task: Option<thread::JoinHandle<()>>,
}
impl EmbedServer {
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|_| "Could not start the local YouTube player origin")?;
        let address = listener
            .local_addr()
            .map_err(|_| "Missing local player address")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Could not configure local player origin")?;
        let prefix = format!("/{}/", uuid::Uuid::now_v7());
        let origin = format!("http://{address}");
        let page = format!("{origin}{prefix}player.html");
        let stop = Arc::new(AtomicBool::new(false));
        let quit = stop.clone();
        let host = address.to_string();
        let task = thread::spawn(move || {
            while !quit.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &host, &prefix),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(15))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            origin,
            page,
            stop,
            task: Some(task),
        })
    }
}
impl Drop for EmbedServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}
fn serve(mut stream: TcpStream, host: &str, prefix: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let mut input = Vec::new();
    let mut buf = [0; 1024];
    while input.len() < 8192 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => input.extend_from_slice(&buf[..n]),
        }
        if input.windows(4).any(|p| p == b"\r\n\r\n") {
            break;
        }
    }
    let request = String::from_utf8_lossy(&input);
    let mut lines = request.lines();
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next();
    let path = parts.next().unwrap_or("");
    let valid_host = lines.any(|line| {
        line.split_once(':')
            .is_some_and(|(name, value)| name.eq_ignore_ascii_case("host") && value.trim() == host)
    });
    let asset = if method == Some("GET") && valid_host {
        path.strip_prefix(prefix)
    } else {
        None
    };
    let (status, kind, body) = match asset {
        Some("player.html") => (
            "200 OK",
            "text/html; charset=utf-8",
            include_str!("../../../../src/youtube/bridge.html"),
        ),
        Some("bridge.js") => (
            "200 OK",
            "text/javascript; charset=utf-8",
            include_str!("../../../../src/youtube/bridge.js"),
        ),
        Some("bridge.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            include_str!("../../../../src/youtube/bridge.css"),
        ),
        _ => ("404 Not Found", "text/plain", "Not found"),
    };
    // Natural HTTP Referer identifies this local wrapper. widget_referrer also
    // identifies our installed application; no unrelated site's identity is used.
    let csp="default-src 'none'; script-src 'self' https://www.youtube.com https://s.ytimg.com; style-src 'self'; frame-src https://www.youtube.com; img-src https://i.ytimg.com; connect-src https://www.youtube.com; frame-ancestors tauri://localhost http://tauri.localhost https://tauri.localhost http://localhost:1420 http://127.0.0.1:1420 http://127.0.0.1:1421";
    let response=format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nContent-Security-Policy: {csp}\r\nReferrer-Policy: strict-origin-when-cross-origin\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",body.len());
    let _ = stream.write_all(response.as_bytes());
}
#[cfg(test)]
mod tests {
    use super::*;
    fn get(server: &EmbedServer, path: &str, host: &str) -> String {
        let address = server.origin.trim_start_matches("http://");
        let mut s = TcpStream::connect(address).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        let mut data = String::new();
        s.read_to_string(&mut data).unwrap();
        data
    }
    #[test]
    fn loopback_assets_require_capability_path_and_exact_host_and_shutdown() {
        let server = EmbedServer::start().unwrap();
        let url = url::Url::parse(&server.page).unwrap();
        let host = server.origin.trim_start_matches("http://");
        let response = get(&server, url.path(), host);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("strict-origin-when-cross-origin"));
        assert!(response.contains("frame-ancestors"));
        assert!(get(&server, "/player.html", host).starts_with("HTTP/1.1 404"));
        assert!(get(&server, url.path(), "attacker.invalid").starts_with("HTTP/1.1 404"));
        assert!(get(&server, "/../Cargo.toml", host).starts_with("HTTP/1.1 404"));
        let address = host.to_owned();
        drop(server);
        assert!(TcpStream::connect(address).is_err());
    }
}
