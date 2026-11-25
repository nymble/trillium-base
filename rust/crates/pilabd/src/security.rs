// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Request guard for the console: Host and Origin checks, and response hardening headers.
//!
//! The pairing token travels in an `Authorization` header (not a cookie), so a web page on
//! another site cannot make an authenticated request on its own. This guard closes the
//! remaining browser-side doors, as defence in depth:
//!
//! * **DNS rebinding**: a hostile page that re-points its own name at the unit's address gets
//!   a request with `Host: evil.example`. Only IP literals, single-label names, the unit's own
//!   host name, `.local`-style private suffixes and names in `PILAB_ALLOWED_HOSTS` pass.
//! * **Cross-site requests**: a state-changing request or a WebSocket upgrade that carries an
//!   `Origin` must come from the same host as the page that was served (or from
//!   `PILAB_ALLOWED_ORIGINS`). Requests without an `Origin` (the CLI, native apps, curl) pass.
//! * **Headers**: `nosniff`, no referrer, no framing, and no caching of API responses.
//!
//! `PILAB_ALLOWED_HOSTS` and `PILAB_ALLOWED_ORIGINS` are comma-separated; an entry may start
//! with `*.` to allow a whole domain (hosts) or be a full origin such as `https://studio.example`.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Name suffixes that only resolve on private networks.
const PRIVATE_SUFFIXES: &[&str] = &[".local", ".localdomain", ".lan", ".home.arpa", ".internal", ".ts.net"];

pub struct Guard {
    own_host: String,
    hosts: Vec<String>,
    origins: Vec<String>,
}

fn list(var: &str) -> Vec<String> {
    std::env::var(var).unwrap_or_default().split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect()
}

impl Guard {
    pub fn new(own_host: &str) -> Guard {
        Guard { own_host: own_host.to_ascii_lowercase(), hosts: list("PILAB_ALLOWED_HOSTS"), origins: list("PILAB_ALLOWED_ORIGINS") }
    }

    #[cfg(test)]
    fn with(own_host: &str, hosts: &[&str], origins: &[&str]) -> Guard {
        Guard {
            own_host: own_host.to_ascii_lowercase(),
            hosts: hosts.iter().map(|s| s.to_string()).collect(),
            origins: origins.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// `host` is the raw `Host` header value, with or without a port.
    pub fn host_ok(&self, host: &str) -> bool {
        let h = host.trim().to_ascii_lowercase();
        let name = if let Some(rest) = h.strip_prefix('[') {
            // [v6]:port
            match rest.split_once(']') {
                Some((ip, _)) => return ip.parse::<std::net::Ipv6Addr>().is_ok(),
                None => return false,
            }
        } else {
            match h.rsplit_once(':') {
                Some((n, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => n.to_string(),
                _ => h.clone(),
            }
        };
        let name = name.trim_end_matches('.');
        if name.is_empty() {
            return false;
        }
        if name.parse::<IpAddr>().is_ok() || name == "localhost" || !name.contains('.') || name == self.own_host {
            return true;
        }
        if PRIVATE_SUFFIXES.iter().any(|s| name.ends_with(s)) {
            return true;
        }
        self.hosts.iter().any(|a| match a.strip_prefix("*.") {
            Some(suffix) => name.ends_with(&format!(".{suffix}")),
            None => a == name,
        })
    }

    /// `origin` is the raw `Origin` header; `host` the request's `Host` header.
    pub fn origin_ok(&self, origin: &str, host: &str) -> bool {
        let o = origin.trim().to_ascii_lowercase();
        if o == "null" {
            return false;
        }
        if self.origins.iter().any(|a| *a == o) {
            return true;
        }
        let authority = o.split_once("://").map(|(_, a)| a).unwrap_or("");
        !authority.is_empty() && authority == host.trim().to_ascii_lowercase()
    }

    /// Reject a request the browser should not have been able to make. `None` means pass.
    pub fn verdict(&self, method: &Method, headers: &HeaderMap) -> Option<&'static str> {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
        if let Some(h) = host {
            if !self.host_ok(h) {
                return Some("unexpected Host header (set PILAB_ALLOWED_HOSTS to allow this name)");
            }
        }
        let upgrade = headers.get(header::UPGRADE).is_some();
        let writes = !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
        if writes || upgrade {
            if let Some(o) = headers.get(header::ORIGIN) {
                let ok = o.to_str().ok().is_some_and(|o| self.origin_ok(o, host.unwrap_or("")));
                if !ok {
                    return Some("cross-origin request refused (set PILAB_ALLOWED_ORIGINS to allow an origin)");
                }
            }
        }
        None
    }
}

pub async fn guard(State(g): State<Arc<Guard>>, req: Request, next: Next) -> Response {
    if let Some(why) = g.verdict(req.method(), req.headers()) {
        let mut r = (StatusCode::FORBIDDEN, Json(json!({ "error": why }))).into_response();
        harden(&mut r, true);
        return r;
    }
    let api = req.uri().path().starts_with("/api/");
    let mut r = next.run(req).await;
    harden(&mut r, api);
    r
}

fn harden(r: &mut Response, api: bool) {
    let h = r.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("frame-ancestors 'none'"));
    if api && !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::{get, post};
    use axum::Router;
    use std::io::{Read, Write};

    #[test]
    fn hosts() {
        let g = Guard::with("tbase-1a2b", &["studio.example.com", "*.corp.test"], &[]);
        for ok in ["pilab.local:8410", "192.168.1.20:8410", "[fe80::1]:8410", "[::1]", "localhost:8410", "tbase", "TBASE-1A2B", "tbase-1a2b.local.", "box.tail1234.ts.net", "studio.example.com:8410", "a.corp.test", "10.0.0.1"] {
            assert!(g.host_ok(ok), "{ok} should pass");
        }
        for bad in ["evil.example.com", "evil.example.com:8410", "pilab.local.evil.com", "tbase-1a2b.example", "corp.test.evil.com", "", ":8410", "[::1", "x.y"] {
            assert!(!g.host_ok(bad), "{bad:?} should be refused");
        }
    }

    #[test]
    fn origins() {
        let g = Guard::with("tbase", &[], &["https://studio.example"]);
        assert!(g.origin_ok("http://pilab.local:8410", "pilab.local:8410"));
        assert!(g.origin_ok("HTTP://PILAB.local:8410", "pilab.LOCAL:8410"));
        assert!(g.origin_ok("https://studio.example", "pilab.local:8410"));
        assert!(!g.origin_ok("http://evil.example", "pilab.local:8410"));
        assert!(!g.origin_ok("null", "pilab.local:8410"));
        assert!(!g.origin_ok("file://", "pilab.local:8410"));
        assert!(!g.origin_ok("http://pilab.local:9999", "pilab.local:8410"));
    }

    fn raw(addr: std::net::SocketAddr, req: String) -> String {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn through_the_router() {
        let g = Arc::new(Guard::with("tbase", &[], &[]));
        let app = Router::new()
            .route("/api/x", get(|| async { "ok" }).post(|| async { "posted" }))
            .route("/page", get(|| async { "page" }))
            .layer(axum::middleware::from_fn_with_state(g, guard));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let h = format!("127.0.0.1:{}", addr.port());
        let ask = move |method: &str, path: &str, host: &str, extra: &str| {
            raw(addr, format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: 0\r\n{extra}\r\n"))
        };
        let (h2, a2) = (h.clone(), ask.clone());
        let r = tokio::task::spawn_blocking(move || {
            let same = a2("POST", "/api/x", &h2, &format!("Origin: http://{h2}\r\n"));
            let none = a2("POST", "/api/x", &h2, "");
            let cross = a2("POST", "/api/x", &h2, "Origin: http://evil.example\r\n");
            let nul = a2("POST", "/api/x", &h2, "Origin: null\r\n");
            let rebind = a2("GET", "/api/x", "evil.example.com:8410", "");
            let read_cross = a2("GET", "/api/x", &h2, "Origin: http://evil.example\r\n");
            let ws_cross = a2("GET", "/api/x", &h2, "Upgrade: websocket\r\nConnection: Upgrade\r\nOrigin: http://evil.example\r\n");
            let page = a2("GET", "/page", &h2, "");
            (same, none, cross, nul, rebind, read_cross, ws_cross, page)
        })
        .await
        .unwrap();
        let lower = |s: &String| s.to_ascii_lowercase();
        assert!(r.0.starts_with("HTTP/1.1 200"), "same-origin post: {}", r.0);
        assert!(r.1.starts_with("HTTP/1.1 200"), "no Origin (CLI): {}", r.1);
        assert!(r.2.starts_with("HTTP/1.1 403"), "cross-origin post: {}", r.2);
        assert!(r.3.starts_with("HTTP/1.1 403"), "null origin: {}", r.3);
        assert!(r.4.starts_with("HTTP/1.1 403"), "rebinding Host: {}", r.4);
        assert!(r.5.starts_with("HTTP/1.1 200"), "plain cross-origin GET is the browser's job (no CORS headers): {}", r.5);
        assert!(r.6.starts_with("HTTP/1.1 403"), "cross-origin websocket upgrade: {}", r.6);
        let ok = lower(&r.0);
        assert!(ok.contains("x-content-type-options: nosniff") && ok.contains("x-frame-options: deny") && ok.contains("referrer-policy: no-referrer"));
        assert!(ok.contains("cache-control: no-store"), "API responses are not cached");
        assert!(!lower(&r.7).contains("cache-control"), "non-API paths keep their own caching");
        assert!(lower(&r.7).contains("frame-ancestors 'none'"));
    }
}
