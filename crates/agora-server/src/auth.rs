//! Who is calling: local trust, pairing codes, device tokens, origin checks, and the
//! network addresses a phone can use to reach this machine.

use agora_core::{model::random_hex, now_ms, store::PairedDevice, Engine};
use axum::http::{header, HeaderMap};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Mutex;
use std::time::Duration;

const CODE_TTL_MS: u64 = 10 * 60_000;
const MAX_FAILURES: usize = 8;
const FAILURE_WINDOW_MS: u64 = 10 * 60_000;

fn host_header(headers: &HeaderMap) -> Option<String> {
    headers.get(header::HOST).and_then(|h| h.to_str().ok()).map(|s| s.to_ascii_lowercase())
}

fn strip_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    host.rsplit_once(':').map_or(host, |(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host })
}

/// Loopback peer, a loopback `Host`, and no proxy headers (so `tailscale serve` and
/// DNS-rebinding requests are *not* local).
pub fn is_local(addr: SocketAddr, headers: &HeaderMap) -> bool {
    if !addr.ip().is_loopback() {
        return false;
    }
    if ["x-forwarded-for", "forwarded", "x-forwarded-host", "tailscale-user-login"].iter().any(|h| headers.contains_key(*h)) {
        return false;
    }
    match host_header(headers) {
        Some(h) => matches!(strip_port(&h), "127.0.0.1" | "localhost" | "::1"),
        None => false,
    }
}

/// `Origin` (when present) must name the same host:port as `Host`; `Sec-Fetch-Site:
/// cross-site` is always refused.
pub fn same_origin(headers: &HeaderMap) -> bool {
    if headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return false;
    }
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    if origin == "null" {
        return false;
    }
    let origin_host = origin.split("://").nth(1).unwrap_or("").trim_end_matches('/').to_ascii_lowercase();
    let host = host_header(headers).unwrap_or_default();
    let fwd = headers.get("x-forwarded-host").and_then(|v| v.to_str().ok()).map(|s| s.to_ascii_lowercase());
    // Exact host:port match only — another app on a different localhost port is a different
    // origin and must not be able to drive this API.
    origin_host == host || fwd.is_some_and(|f| f == origin_host)
}

pub fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("application/json"))
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).find_map(|c| {
        c.split(';').find_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            (k == "agora_token").then(|| v.to_string())
        })
    })
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Name of the paired device presenting a valid token, if any.
pub fn device_for(engine: &Engine, headers: &HeaderMap) -> Option<String> {
    let token = cookie_token(headers)?;
    let s = engine.settings();
    let d = s.devices.iter().find(|d| ct_eq(&d.token, &token))?;
    let (id, name, stale) = (d.id.clone(), d.name.clone(), now_ms().saturating_sub(d.last_seen) > 10 * 60_000);
    if stale {
        let _ = engine.update_settings(|s| {
            if let Some(d) = s.devices.iter_mut().find(|d| d.id == id) {
                d.last_seen = now_ms();
            }
        });
    }
    Some(name)
}

pub fn register_device(engine: &Engine, name: &str) -> anyhow::Result<String> {
    let token = random_hex(32);
    let now = now_ms();
    let dev = PairedDevice { id: agora_core::new_id("d"), name: name.into(), token: token.clone(), created_at: now, last_seen: now };
    engine.update_settings(|s| s.devices.push(dev))?;
    Ok(token)
}

struct PairState {
    code: String,
    expires_at: u64,
    failures: Vec<u64>,
}

pub struct Pairing {
    inner: Mutex<PairState>,
}

fn fresh_code() -> String {
    let n = u32::from_str_radix(&random_hex(4), 16).unwrap_or(0) % 1_000_000;
    format!("{n:06}")
}

impl Default for Pairing {
    fn default() -> Self {
        Self::new()
    }
}

impl Pairing {
    pub fn new() -> Self {
        Pairing { inner: Mutex::new(PairState { code: fresh_code(), expires_at: now_ms() + CODE_TTL_MS, failures: vec![] }) }
    }

    pub fn current(&self) -> (String, u64) {
        let mut s = self.inner.lock().unwrap();
        if now_ms() >= s.expires_at {
            s.code = fresh_code();
            s.expires_at = now_ms() + CODE_TTL_MS;
        }
        (s.code.clone(), s.expires_at)
    }

    pub fn rotate(&self) {
        let mut s = self.inner.lock().unwrap();
        s.code = fresh_code();
        s.expires_at = now_ms() + CODE_TTL_MS;
    }

    pub fn try_pair(&self, code: &str) -> Result<(), String> {
        let mut s = self.inner.lock().unwrap();
        let now = now_ms();
        s.failures.retain(|t| now.saturating_sub(*t) < FAILURE_WINDOW_MS);
        if s.failures.len() >= MAX_FAILURES {
            return Err("嘗試次數太多，請 10 分鐘後再試".into());
        }
        if now >= s.expires_at {
            s.code = fresh_code();
            s.expires_at = now + CODE_TTL_MS;
            s.failures.push(now);
            return Err("配對碼已過期，請看電腦上的新配對碼".into());
        }
        if ct_eq(code.trim(), &s.code) {
            s.code = fresh_code();
            s.expires_at = now + CODE_TTL_MS;
            Ok(())
        } else {
            s.failures.push(now);
            Err("配對碼不正確".into())
        }
    }
}

/// Primary LAN address (UDP "connect" picks the outbound interface; nothing is sent).
pub fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:80").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

/// `(tailscale IPv4, MagicDNS name)` when the Tailscale CLI is installed and up.
pub async fn tailscale_info() -> Option<(String, Option<String>)> {
    let exe = agora_core::provider::resolve::find_in_path(&["tailscale.exe", "tailscale"]).or_else(|| {
        let p = std::path::PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe");
        p.is_file().then_some(p)
    })?;
    let run = |args: &'static [&'static str]| {
        let exe = exe.clone();
        async move {
            let mut c = tokio::process::Command::new(exe);
            c.args(args).stdin(std::process::Stdio::null()).kill_on_drop(true);
            agora_core::provider::proc::no_window(&mut c);
            let out = tokio::time::timeout(Duration::from_secs(5), c.output()).await.ok()?.ok()?;
            out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        }
    };
    let ip = run(&["ip", "-4"]).await?.lines().next()?.to_string();
    let dns = run(&["status", "--json"])
        .await
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.pointer("/Self/DNSName").and_then(|d| d.as_str()).map(str::to_string))
        .filter(|d| !d.is_empty());
    Some((ip, dns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn h(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut m = HeaderMap::new();
        for (k, v) in pairs {
            m.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        m
    }

    #[test]
    fn local_requires_loopback_host_and_no_proxy() {
        let lo: SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let lan: SocketAddr = "192.168.1.9:5555".parse().unwrap();
        assert!(is_local(lo, &h(&[("host", "127.0.0.1:4717")])));
        assert!(is_local(lo, &h(&[("host", "localhost:4717")])));
        assert!(!is_local(lan, &h(&[("host", "127.0.0.1:4717")])));
        // DNS rebinding: attacker domain resolving to 127.0.0.1.
        assert!(!is_local(lo, &h(&[("host", "evil.example:4717")])));
        // `tailscale serve` proxies from loopback but is remote.
        assert!(!is_local(lo, &h(&[("host", "pc.tail1234.ts.net"), ("tailscale-user-login", "me@x")])));
    }

    #[test]
    fn origin_must_match_host() {
        assert!(same_origin(&h(&[("host", "127.0.0.1:4717"), ("origin", "http://127.0.0.1:4717")])));
        assert!(!same_origin(&h(&[("host", "127.0.0.1:4717"), ("origin", "https://evil.example")])));
        assert!(!same_origin(&h(&[("host", "127.0.0.1:4717"), ("origin", "http://127.0.0.1:8080")])), "other local port");
        assert!(!same_origin(&h(&[("host", "127.0.0.1:4717"), ("sec-fetch-site", "cross-site")])));
        assert!(same_origin(&h(&[("host", "127.0.0.1:4717")])), "non-browser clients send no Origin");
        assert!(same_origin(&h(&[("host", "127.0.0.1:4717"), ("x-forwarded-host", "pc.ts.net"), ("origin", "https://pc.ts.net")])));
    }

    #[test]
    fn pairing_code_is_single_use_and_rate_limited() {
        let p = Pairing::new();
        let (code, _) = p.current();
        assert!(p.try_pair("000000x").is_err());
        assert!(p.try_pair(&code).is_ok());
        assert!(p.try_pair(&code).is_err(), "rotated after use");
        for _ in 0..MAX_FAILURES {
            let _ = p.try_pair("bad");
        }
        let (fresh, _) = p.current();
        assert!(p.try_pair(&fresh).unwrap_err().contains("太多"));
    }

    #[test]
    fn cookie_parsing() {
        let m = h(&[("cookie", "a=1; agora_token=abc; b=2")]);
        assert_eq!(cookie_token(&m).as_deref(), Some("abc"));
    }
}
