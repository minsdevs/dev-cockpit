//! Service health probes: TCP connect + minimal HTTP status check.
//! Zero-dependency (std::net); localhost dev servers only.
//!
//! Protocol strategy (P0 safety improvement):
//!   - Default: TCP-only (verify port accepts connections)
//!   - HTTP: only when framework detection confirms HTTP or user explicitly configures it
//!   - None: skip checks entirely for known non-HTTP internal ports

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const TCP_TIMEOUT: Duration = Duration::from_millis(400);
const HTTP_TIMEOUT: Duration = Duration::from_millis(900);

/// Result of a single health check cycle.
#[derive(Debug, Clone)]
pub struct HealthResult {
    pub tcp_ok: bool,
    pub http_status: Option<u16>,
    /// "tcp", "http", or "none"
    pub method_used: &'static str,
    /// Human-readable failure reason, if any.
    pub failure_reason: Option<String>,
}

fn connect(port: u16) -> Option<TcpStream> {
    let v4: SocketAddr = ([127, 0, 0, 1], port).into();
    if let Ok(s) = TcpStream::connect_timeout(&v4, TCP_TIMEOUT) {
        return Some(s);
    }
    let v6: SocketAddr = (std::net::Ipv6Addr::LOCALHOST, port).into();
    TcpStream::connect_timeout(&v6, TCP_TIMEOUT).ok()
}

/// Can we open a TCP connection to localhost:port?
pub fn tcp_check(port: u16) -> bool {
    connect(port).is_some()
}

/// Issue `GET /` and return the HTTP status code, if the peer speaks HTTP.
/// Any well-formed response (including 404/500) proves the server is alive.
pub fn http_check(port: u16) -> Option<u16> {
    http_check_path(port, "GET", "/")
}

/// Issue an HTTP request to a specific path and return the status code.
/// Supports configurable method and path for per-service health checks.
pub fn http_check_path(port: u16, method: &str, path: &str) -> Option<u16> {
    let mut stream = connect(port)?;
    stream.set_read_timeout(Some(HTTP_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(HTTP_TIMEOUT)).ok()?;
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nUser-Agent: dev-cockpit\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = [0u8; 512];
    let n = stream.read(&mut buf).ok()?;
    parse_status_line(&buf[..n])
}

/// Perform a full health check using the resolved configuration.
/// `method`: "tcp", "http", or "none"
/// Returns a structured result with failure reasoning.
pub fn check_service(
    port: u16,
    method: &str,
    http_method: &str,
    http_path: &str,
    expected_status: &[u16],
) -> HealthResult {
    match method {
        "none" => HealthResult {
            tcp_ok: true, // assume ok when checks disabled
            http_status: None,
            method_used: "none",
            failure_reason: None,
        },
        "http" => {
            let tcp_ok = tcp_check(port);
            if !tcp_ok {
                return HealthResult {
                    tcp_ok: false,
                    http_status: None,
                    method_used: "http",
                    failure_reason: Some("port not accepting connections".into()),
                };
            }
            let status = http_check_path(port, http_method, http_path);
            let failure_reason = match status {
                None => Some(format!(
                    "no HTTP response on {http_method} {http_path} (non-HTTP protocol?)"
                )),
                Some(code) if !expected_status.is_empty() && !expected_status.contains(&code) => {
                    Some(format!(
                        "HTTP {code} (expected one of {:?})",
                        expected_status
                    ))
                }
                Some(code) if code >= 500 => Some(format!("HTTP {code} server error")),
                _ => None,
            };
            HealthResult {
                tcp_ok: true,
                http_status: status,
                method_used: "http",
                failure_reason,
            }
        }
        _ => {
            // "tcp" or any unknown → TCP only
            let tcp_ok = tcp_check(port);
            HealthResult {
                tcp_ok,
                http_status: None,
                method_used: "tcp",
                failure_reason: if tcp_ok {
                    None
                } else {
                    Some("port not accepting connections".into())
                },
            }
        }
    }
}

/// Resolve the effective check method for a service.
/// Priority: explicit config > auto (based on is_http flag).
/// "auto" + is_http=true → "http"; "auto" + is_http=false → "tcp".
pub fn resolve_method(config_method: &str, is_http: bool) -> &'static str {
    match config_method {
        "http" => "http",
        "tcp" => "tcp",
        "none" => "none",
        _ => {
            // "auto" or anything else
            if is_http {
                "http"
            } else {
                "tcp"
            }
        }
    }
}

/// Calculate backoff interval multiplier based on consecutive failures.
/// Returns a multiplier (1, 2, 4, 8, max 10) for the check interval.
pub fn backoff_multiplier(consecutive_failures: u32) -> u32 {
    match consecutive_failures {
        0..=2 => 1,
        3..=4 => 2,
        5..=6 => 4,
        7..=8 => 8,
        _ => 10,
    }
}

pub fn parse_status_line(buf: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(buf).ok()?;
    let line = text.lines().next()?;
    if !line.starts_with("HTTP/") {
        return None;
    }
    line.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parses_status_lines() {
        assert_eq!(parse_status_line(b"HTTP/1.1 200 OK\r\n"), Some(200));
        assert_eq!(parse_status_line(b"HTTP/1.0 404 Not Found\r\n"), Some(404));
        assert_eq!(parse_status_line(b"SSH-2.0-OpenSSH"), None);
        assert_eq!(parse_status_line(&[0xff, 0xfe]), None);
    }

    #[test]
    fn tcp_and_http_against_local_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf);
                let _ = sock.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
            }
        });
        assert!(tcp_check(port));
        assert_eq!(http_check(port), Some(204));
        assert!(!tcp_check(1));
    }

    #[test]
    fn http_check_path_custom() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let n = sock.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                if req.contains("/health") {
                    let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                } else {
                    let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
                }
            }
        });
        assert_eq!(http_check_path(port, "GET", "/health"), Some(200));
        assert_eq!(http_check_path(port, "GET", "/missing"), Some(404));
    }

    #[test]
    fn check_service_tcp_only() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Keep the listener alive so TCP checks succeed
        std::thread::spawn(move || {
            while let Ok((_sock, _)) = listener.accept() {
                // just accept
            }
        });
        let result = check_service(port, "tcp", "GET", "/", &[]);
        assert!(result.tcp_ok);
        assert_eq!(result.method_used, "tcp");
        assert!(result.http_status.is_none());
        assert!(result.failure_reason.is_none());
    }

    #[test]
    fn check_service_tcp_sends_no_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_millis(500)))
                .unwrap();
            let mut bytes = [0u8; 16];
            let read = socket.read(&mut bytes).unwrap_or(0);
            tx.send(read).unwrap();
        });
        let result = check_service(port, "tcp", "GET", "/", &[]);
        assert!(result.tcp_ok);
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap(),
            0
        );
    }

    #[test]
    fn check_service_none_skips() {
        let result = check_service(1, "none", "GET", "/", &[]);
        assert!(result.tcp_ok);
        assert_eq!(result.method_used, "none");
        assert!(result.failure_reason.is_none());
    }

    #[test]
    fn check_service_tcp_failure() {
        // Port 1 is unlikely to be open
        let result = check_service(1, "tcp", "GET", "/", &[]);
        assert!(!result.tcp_ok);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("port not accepting connections")
        );
    }

    #[test]
    fn resolve_method_logic() {
        assert_eq!(resolve_method("auto", true), "http");
        assert_eq!(resolve_method("auto", false), "tcp");
        assert_eq!(resolve_method("tcp", true), "tcp");
        assert_eq!(resolve_method("http", false), "http");
        assert_eq!(resolve_method("none", true), "none");
    }

    #[test]
    fn backoff_multiplier_values() {
        assert_eq!(backoff_multiplier(0), 1);
        assert_eq!(backoff_multiplier(2), 1);
        assert_eq!(backoff_multiplier(3), 2);
        assert_eq!(backoff_multiplier(5), 4);
        assert_eq!(backoff_multiplier(7), 8);
        assert_eq!(backoff_multiplier(10), 10);
    }

    #[test]
    fn check_service_http_with_expected_status() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf);
                let _ = sock.write_all(b"HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n");
            }
        });
        // 401 is fine when we don't restrict expected_status
        let result = check_service(port, "http", "GET", "/", &[]);
        assert!(result.tcp_ok);
        assert_eq!(result.http_status, Some(401));
        assert!(result.failure_reason.is_none()); // 401 < 500, no expected list

        // But fails when we expect only 200
        let result2 = check_service(port, "http", "GET", "/", &[200]);
        assert_eq!(result2.http_status, Some(401));
        assert!(result2.failure_reason.is_some());
    }
}
