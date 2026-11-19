//! "Sign in with Google" through the system browser, for desktop and
//! Android. The app listens on a free port of 127.0.0.1, opens
//! `<server>/auth/google?return=app&port=…&state=…`, and the server sends the
//! browser back to `http://127.0.0.1:<port>/callback?token=…&state=…` once
//! Google has signed the person in. `state` ties the answer to this sign-in.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

/// Starts a browser sign-in. `open` shows the address in a browser; `done`
/// gets the session token or a message, on a background thread. `back` is an
/// optional link for the page the browser lands on, such as one that returns
/// to the app on Android. Returns false if it couldn't start.
pub fn start(
    server: &str,
    back: Option<String>,
    open: impl FnOnce(&str) -> bool,
    done: impl FnOnce(Result<String, String>) + Send + 'static,
) -> bool {
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else { return false };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else { return false };
    let state = random_state();
    let url = format!("{}/auth/google?return=app&port={port}&state={state}", server.trim_end_matches('/'));
    if !open(&url) {
        return false;
    }
    std::thread::Builder::new()
        .name("browser-sign-in".into())
        .spawn(move || done(wait(listener, &state, back.as_deref())))
        .is_ok()
}

/// A hard-to-guess value tying the browser's answer to this sign-in.
fn random_state() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i ^ crate::sys::now_ms() as u64);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(v) => {
                    out.push(v);
                    i += 2;
                }
                Err(_) => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Answers the browser's request to /callback (for up to 10 minutes).
fn wait(listener: TcpListener, state: &str, back: Option<&str>) -> Result<String, String> {
    let _ = listener.set_nonblocking(true);
    let deadline = Instant::now() + Duration::from_secs(600);
    while Instant::now() < deadline {
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let head = String::from_utf8_lossy(&buf[..n]);
        let target = head.split_whitespace().nth(1).unwrap_or("");
        let Some(query) = target.strip_prefix("/callback?") else {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        };
        let params: HashMap<String, String> =
            query.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), percent_decode(v))).collect();
        let result = if params.get("state").map(String::as_str) != Some(state) {
            Err("Sign-in didn't start here. Try again.".to_string())
        } else if let Some(token) = params.get("token").filter(|t| !t.is_empty()) {
            Ok(token.clone())
        } else {
            Err(params.get("error").cloned().unwrap_or_else(|| "Sign-in failed. Try again.".into()))
        };
        let (title, line) = match &result {
            Ok(_) => ("You're signed in", "Go back to Inkboard."),
            Err(e) => ("Sign-in didn't work", e.as_str()),
        };
        let button = back.map_or(String::new(), |href| {
            format!("<p><a href=\"{}\" style='display:inline-block;margin-top:12px;padding:14px 22px;border-radius:12px;background:#2748e8;color:#fff;text-decoration:none;font-weight:600'>Back to Inkboard</a></p>", escape(href))
        });
        let body = format!(
            "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'><title>Inkboard</title>\
             <body style='font:18px system-ui;display:grid;place-items:center;height:90vh;margin:0;background:#f7f6fb;color:#1b1a22'>\
             <div style='text-align:center;padding:16px'><h1 style='font-size:28px'>{}</h1><p>{}</p>{button}</div>",
            escape(title),
            escape(line)
        );
        let _ = stream.write_all(
            format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                .as_bytes(),
        );
        return result;
    }
    Err("Sign-in timed out. Try again.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn get(url: &str) -> String {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, path) = rest.split_once('/').unwrap();
        let mut s = std::net::TcpStream::connect(host).unwrap();
        write!(s, "GET /{path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn hands_the_token_back_only_for_this_sign_in() {
        let (tx, rx) = mpsc::channel();
        let (url_tx, url_rx) = mpsc::channel();
        assert!(start("https://ink.example", Some("intent://back".into()), |u| url_tx.send(u.to_string()).is_ok(), move |r| tx.send(r).unwrap()));
        let url = url_rx.recv().unwrap();
        let q: HashMap<_, _> = url.split_once('?').unwrap().1.split('&').filter_map(|kv| kv.split_once('=')).collect();
        assert!(url.starts_with("https://ink.example/auth/google?return=app&port="));
        let port = q["port"];
        // A wrong state is refused; the right one carries the token.
        let page = get(&format!("http://127.0.0.1:{port}/callback?token=t0k&state=nope"));
        assert!(page.contains("didn&#39;t") || page.contains("didn't"));
        assert_eq!(rx.recv().unwrap(), Err("Sign-in didn't start here. Try again.".into()));

        let (tx, rx) = mpsc::channel();
        let (url_tx, url_rx) = mpsc::channel();
        start("https://ink.example", Some("intent://back".into()), |u| url_tx.send(u.to_string()).is_ok(), move |r| tx.send(r).unwrap());
        let url = url_rx.recv().unwrap();
        let q: HashMap<_, _> = url.split_once('?').unwrap().1.split('&').filter_map(|kv| kv.split_once('=')).collect();
        let page = get(&format!("http://127.0.0.1:{}/callback?token=abc%2Ddef&state={}", q["port"], q["state"]));
        assert!(page.contains("You're signed in") && page.contains("intent://back"));
        assert_eq!(rx.recv().unwrap(), Ok("abc-def".into()));
    }
}
