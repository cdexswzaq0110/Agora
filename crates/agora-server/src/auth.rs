//! Who is calling: local trust, pairing codes, device tokens, origin checks, and the
//! network addresses a phone can use to reach this machine.

use agora_core::{model::random_hex, now_ms, store::PairedDevice, Engine};
use axum::http::{header, HeaderMap};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
#[cfg(windows)]
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

// ---------------------------------------------------------------- LAN candidates (phone QR)
// Implemented by GPT (Codex CLI) in an isolated worktree, reviewed and merged by Claude with
// three fixes: ASCII-only PowerShell output, more virtual-adapter markers, a 30 s cache.

/// One private IPv4 address of this machine, with the adapter's name, for the QR code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LanCandidate {
    pub ip: Ipv4Addr,
    pub name: String,
    #[serde(rename = "virtual")]
    pub virtual_adapter: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WindowsAddress {
    interface_alias: String,
    #[serde(rename = "IPAddress")]
    ip_address: String,
}

fn private_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

/// Adapters a phone on the Wi-Fi can never reach: hypervisor / WSL switches, VPN overlays,
/// Bluetooth PAN, and Wi-Fi Direct ("Local Area Connection* 10" / 「區域連線* 10」).
pub fn is_virtual_adapter(name: &str) -> bool {
    let lower = name.to_lowercase();
    let wifi_direct = lower.split("* ").nth(1).is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    wifi_direct
        || ["vethernet", "wsl", "hyper-v", "virtualbox", "vmware", "vmnet", "loopback", "tailscale", "zerotier", "bluetooth", "藍牙", "藍芽"]
            .iter()
            .any(|marker| lower.contains(marker))
}

pub fn parse_windows_lan_candidates(output: &[u8]) -> Vec<LanCandidate> {
    let Ok(text) = std::str::from_utf8(output) else { return vec![] };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else { return vec![] };
    let values = match value {
        serde_json::Value::Array(values) => values,
        serde_json::Value::Object(_) => vec![value],
        _ => return vec![],
    };
    let mut candidates = Vec::new();
    for value in values {
        let Ok(address) = serde_json::from_value::<WindowsAddress>(value) else { continue };
        let Ok(ip) = address.ip_address.parse::<Ipv4Addr>() else { continue };
        if !private_ipv4(ip) || candidates.iter().any(|candidate: &LanCandidate| candidate.ip == ip) {
            continue;
        }
        candidates.push(LanCandidate { ip, virtual_adapter: is_virtual_adapter(&address.interface_alias), name: address.interface_alias });
    }
    candidates
}

/// The default-route address, unless it is a virtual adapter and a physical one exists.
pub fn select_lan_candidate(candidates: &[LanCandidate], default: Option<IpAddr>) -> Option<LanCandidate> {
    let default = match default {
        Some(IpAddr::V4(ip)) => candidates.iter().find(|candidate| candidate.ip == ip),
        _ => None,
    };
    match default {
        Some(candidate) if candidate.virtual_adapter => candidates.iter().find(|candidate| !candidate.virtual_adapter).or(Some(candidate)).cloned(),
        Some(candidate) => Some(candidate.clone()),
        None => candidates.iter().find(|candidate| !candidate.virtual_adapter).or(candidates.first()).cloned(),
    }
}

fn fallback_lan_candidates(default: Option<IpAddr>) -> Vec<LanCandidate> {
    match default {
        Some(IpAddr::V4(ip)) if private_ipv4(ip) => vec![LanCandidate { ip, name: "區網".into(), virtual_adapter: false }],
        _ => vec![],
    }
}

/// Windows PowerShell 5.1 writes stdout in the console code page (cp950 on zh-TW), so adapter
/// names like 「乙太網路」 are not UTF-8 and the whole list failed to parse. Escape every
/// non-ASCII character as a JSON `\uXXXX` inside PowerShell: the output is pure ASCII on any
/// locale and serde restores the names exactly.
#[cfg(windows)]
const PS_LIST_IPV4: &str = r"$j = Get-NetIPAddress -AddressFamily IPv4 | Select-Object InterfaceAlias,IPAddress | ConvertTo-Json -Compress; [regex]::Replace([string]$j, '[^\x00-\x7F]', { param($m) '\u{0:x4}' -f [int][char]$m.Value })";

static LAN_CACHE: Mutex<Option<(Instant, Vec<LanCandidate>)>> = Mutex::new(None);
const LAN_CACHE_TTL: Duration = Duration::from_secs(30);

/// Private IPv4 addresses of this machine (cached 30 s: PowerShell takes about a second to
/// start, and the settings page and the QR image both ask).
pub async fn lan_candidates() -> Vec<LanCandidate> {
    if let Some((at, v)) = LAN_CACHE.lock().unwrap().clone() {
        if at.elapsed() < LAN_CACHE_TTL {
            return v;
        }
    }
    let list = discover_lan_candidates().await;
    *LAN_CACHE.lock().unwrap() = Some((Instant::now(), list.clone()));
    list
}

async fn discover_lan_candidates() -> Vec<LanCandidate> {
    let default = lan_ip();
    #[cfg(windows)]
    {
        let mut command = agora_core::provider::proc::command(Path::new("powershell.exe"));
        command.args(["-NoProfile", "-NonInteractive", "-Command", PS_LIST_IPV4]);
        if let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(5), command.output()).await {
            if output.status.success() {
                let candidates = parse_windows_lan_candidates(&output.stdout);
                if !candidates.is_empty() {
                    return candidates;
                }
            }
        }
    }
    fallback_lan_candidates(default)
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

// ---------------------------------------------------------------- tailscale serve (v0.5)
// v0.4 assumed `https://<host>.ts.net/` (port 443) and suggested `tailscale serve --bg 4717`.
// On a machine that already serves something else on 443 that URL is another app, and the
// suggested command would replace the user's mapping. Read the real serve config instead and
// only ever *add* a mapping on a free HTTPS port.

fn tailscale_exe() -> Option<std::path::PathBuf> {
    agora_core::provider::resolve::find_in_path(&["tailscale.exe", "tailscale"]).or_else(|| {
        let p = std::path::PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe");
        p.is_file().then_some(p)
    })
}

async fn tailscale_run(args: &[&str], secs: u64) -> Result<String, String> {
    let exe = tailscale_exe().ok_or("找不到 Tailscale CLI")?;
    let mut c = tokio::process::Command::new(exe);
    c.args(args).stdin(std::process::Stdio::null()).kill_on_drop(true);
    agora_core::provider::proc::no_window(&mut c);
    let out = tokio::time::timeout(Duration::from_secs(secs), c.output()).await.map_err(|_| "Tailscale CLI 逾時".to_string())?.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// `http://127.0.0.1:4717`, `localhost:4717/`, `4717` → does it point at this port on loopback?
fn proxies_to(target: &str, port: u16) -> bool {
    let t = target.trim().trim_start_matches("http://").trim_end_matches('/');
    let (host, p) = t.rsplit_once(':').unwrap_or(("127.0.0.1", t));
    matches!(host, "127.0.0.1" | "localhost") && p.parse::<u16>().ok() == Some(port)
}

/// From `tailscale serve status --json`: the HTTPS URL whose `/` handler proxies to Agora.
pub fn parse_serve_url(json: &str, port: u16) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let web = v.get("Web")?.as_object()?;
    let mut hits: Vec<(String, u16)> = web
        .iter()
        .filter(|(_, conf)| conf.pointer("/Handlers/~1/Proxy").and_then(|p| p.as_str()).is_some_and(|p| proxies_to(p, port)))
        .filter_map(|(host_port, _)| {
            let (host, p) = host_port.rsplit_once(':')?;
            Some((host.trim_end_matches('.').to_string(), p.parse().ok()?))
        })
        .collect();
    hits.sort_by_key(|(_, p)| *p);
    hits.first().map(|(host, p)| if *p == 443 { format!("https://{host}/") } else { format!("https://{host}:{p}/") })
}

/// HTTPS ports the serve config already uses (any handler): never touched by Agora.
pub fn used_serve_ports(json: &str) -> Vec<u16> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let mut ports: Vec<u16> = v.get("TCP").and_then(|t| t.as_object()).map(|m| m.keys().filter_map(|k| k.parse().ok()).collect()).unwrap_or_default();
    if let Some(web) = v.get("Web").and_then(|w| w.as_object()) {
        ports.extend(web.keys().filter_map(|k| k.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok())));
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// The ports Tailscale serves HTTPS on without extra setup, first free one wins.
pub fn pick_serve_port(used: &[u16]) -> Option<u16> {
    [443u16, 8443, 10000].into_iter().find(|p| !used.contains(p))
}

/// `Some(url)` when Tailscale already serves Agora over HTTPS.
pub async fn tailscale_serve_url(port: u16) -> Option<String> {
    let json = tailscale_run(&["serve", "status", "--json"], 5).await.ok()?;
    parse_serve_url(&json, port)
}

/// Add (never replace) an HTTPS mapping to Agora on a free port; returns the URL.
pub async fn enable_tailscale_serve(port: u16) -> Result<String, String> {
    let json = tailscale_run(&["serve", "status", "--json"], 5).await.unwrap_or_default();
    if let Some(url) = parse_serve_url(&json, port) {
        return Ok(url);
    }
    let https = pick_serve_port(&used_serve_ports(&json)).ok_or("Tailscale 的 HTTPS 連接埠（443、8443、10000）都已被其他服務使用")?;
    let target = format!("http://127.0.0.1:{port}");
    let flag = format!("--https={https}");
    tailscale_run(&["serve", "--bg", &flag, &target], 30).await.map_err(|e| format!("tailscale serve 失敗：{e}"))?;
    let after = tailscale_run(&["serve", "status", "--json"], 5).await.unwrap_or_default();
    parse_serve_url(&after, port).ok_or_else(|| "已執行 tailscale serve，但讀不到對應的網址".into())
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

    fn candidate(ip: &str, name: &str) -> LanCandidate {
        LanCandidate { ip: ip.parse().unwrap(), name: name.into(), virtual_adapter: is_virtual_adapter(name) }
    }

    #[test]
    fn parses_single_windows_address() {
        assert_eq!(parse_windows_lan_candidates(br#"{"InterfaceAlias":"Wi-Fi","IPAddress":"192.168.1.8"}"#), vec![candidate("192.168.1.8", "Wi-Fi")]);
    }

    #[test]
    fn parses_array_and_filters_non_private_addresses() {
        let candidates = parse_windows_lan_candidates(br#"[{"InterfaceAlias":"Ethernet","IPAddress":"10.0.0.2"},{"InterfaceAlias":"vEthernet (WSL)","IPAddress":"172.24.0.1"},{"InterfaceAlias":"Public","IPAddress":"8.8.8.8"},{"InterfaceAlias":"Link","IPAddress":"169.254.1.2"}]"#);
        assert_eq!(candidates, vec![candidate("10.0.0.2", "Ethernet"), candidate("172.24.0.1", "vEthernet (WSL)")]);
        assert!(candidates[1].virtual_adapter);
    }

    #[test]
    fn ascii_escaped_chinese_adapter_names_round_trip() {
        // Adapter names measured on a zh-TW machine: 乙太網路, 區域連線* 10, 藍牙網路連線.
        let json = r#"[{"InterfaceAlias":"乙太網路","IPAddress":"192.168.1.20"},{"InterfaceAlias":"區域連線* 10","IPAddress":"192.168.137.1"},{"InterfaceAlias":"藍牙網路連線","IPAddress":"10.1.2.3"},{"InterfaceAlias":"Wi-Fi","IPAddress":"192.168.50.7"}]"#;
        // Escape exactly as PS_LIST_IPV4 does: every non-ASCII char → backslash, 'u', 4 hex digits.
        let backslash = char::from(92u8);
        let out: String = json.chars().map(|ch| if ch.is_ascii() { ch.to_string() } else { format!("{backslash}u{:04x}", ch as u32) }).collect();
        assert!(out.is_ascii());
        let c = parse_windows_lan_candidates(out.as_bytes());
        assert_eq!(c.iter().map(|x| (x.name.as_str(), x.virtual_adapter)).collect::<Vec<_>>(), vec![("乙太網路", false), ("區域連線* 10", true), ("藍牙網路連線", true), ("Wi-Fi", false)]);
        assert!(parse_windows_lan_candidates(&[0xa5, 0xd2, 0xa4, 0xd3]).is_empty(), "raw cp950 bytes are not UTF-8: the reason for the escaping");
    }

    #[test]
    fn virtual_adapter_markers() {
        for v in ["vEthernet (Default Switch)", "VMware Network Adapter VMnet8", "Local Area Connection* 10", "區域連線* 1", "Bluetooth Network Connection", "Tailscale", "Loopback Pseudo-Interface 1"] {
            assert!(is_virtual_adapter(v), "{v}");
        }
        for p in ["Wi-Fi", "乙太網路", "Ethernet 2", "區域連線", "Local Area Connection"] {
            assert!(!is_virtual_adapter(p), "{p}");
        }
    }

    #[test]
    fn empty_and_malformed_windows_output_are_ignored() {
        assert!(parse_windows_lan_candidates(b"").is_empty());
        assert!(parse_windows_lan_candidates(&[0xff, 0xfe]).is_empty());
        assert!(parse_windows_lan_candidates(b"not json").is_empty());
    }

    #[test]
    fn selection_keeps_only_virtual_candidate() {
        let candidates = vec![candidate("172.24.0.1", "vEthernet (WSL)")];
        assert_eq!(select_lan_candidate(&candidates, Some("172.24.0.1".parse().unwrap())), Some(candidates[0].clone()));
    }

    #[test]
    fn serve_url_comes_from_the_real_mapping_and_never_assumes_443() {
        // Measured on the user's machine: 443 serves another app, Agora was added on 8443.
        let json = r#"{"TCP":{"443":{"HTTPS":true},"8443":{"HTTPS":true}},"Web":{"desktop-x.tailnet-example.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:18770"}}},"desktop-x.tailnet-example.ts.net:8443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:4717"}}}}}"#;
        assert_eq!(parse_serve_url(json, 4717).as_deref(), Some("https://desktop-x.tailnet-example.ts.net:8443/"));
        assert_eq!(parse_serve_url(json, 4799), None, "no mapping → no URL (v0.4 guessed :443, which is another app here)");
        assert_eq!(used_serve_ports(json), vec![443, 8443]);
        assert_eq!(pick_serve_port(&[443]), Some(8443), "never replace the user's 443 mapping");
        assert_eq!(pick_serve_port(&[]), Some(443));
        assert_eq!(pick_serve_port(&[443, 8443, 10000]), None);
        let only443 = r#"{"Web":{"pc.ts.net:443":{"Handlers":{"/":{"Proxy":"localhost:4717/"}}}}}"#;
        assert_eq!(parse_serve_url(only443, 4717).as_deref(), Some("https://pc.ts.net/"));
        assert!(proxies_to("4717", 4717) && proxies_to("http://localhost:4717", 4717) && !proxies_to("http://10.0.0.2:4717", 4717));
        assert_eq!(parse_serve_url("not json", 4717), None);
    }

    #[test]
    fn selection_replaces_virtual_default_with_physical_candidate() {
        let candidates = vec![candidate("172.24.0.1", "vEthernet (WSL)"), candidate("192.168.1.8", "Wi-Fi")];
        assert_eq!(select_lan_candidate(&candidates, Some("172.24.0.1".parse().unwrap())), Some(candidates[1].clone()));
    }
}
