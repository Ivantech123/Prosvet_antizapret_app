//! Разблокировка ИИ-сервисов (Gemini, ChatGPT, Claude…), закрытых геоблоком
//! со стороны самих сервисов. DPI-обход тут не помогает — нужен зарубежный IP.
//!
//! Как устроено:
//! * Prosvet поднимает маленький DNS-сервер на 127.0.0.153:53;
//! * правила NRPT Windows направляют к нему ТОЛЬКО домены из [`DOMAINS`];
//! * сервер спрашивает «умный DNS» (Comss, GeoHide) по DoH — тот отдаёт адрес
//!   зарубежного прозрачного SNI-прокси. TLS остаётся сквозным: прокси видит
//!   только имя домена, но не содержимое;
//! * AAAA и HTTPS-записи гасятся, иначе браузер уйдёт напрямую по IPv6.
//!
//! Правила NRPT снимаются при выходе — без Prosvet эти домены резолвятся как обычно.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

use crate::engine::Log;

pub const LISTEN: &str = "127.0.0.153";
const NRPT_COMMENT: &str = "Prosvet AI";

/// Умные DNS с прозрачными SNI-прокси. Xbox DNS намеренно нет — его заблокировали.
pub const PROVIDERS: &[(&str, &str, &str)] = &[
    ("comss", "Comss", "https://dns.comss.one/dns-query"),
    ("geohide", "GeoHide", "https://dns.geohide.ru/dns-query"),
];

/// Домены ИИ-сервисов. Ведущая точка — вместе со всеми поддоменами.
pub const DOMAINS: &[&str] = &[
    // Google Gemini / AI Studio / NotebookLM
    "gemini.google.com",
    "aistudio.google.com",
    "notebooklm.google.com",
    "notebooklm.google",
    "labs.google",
    "generativelanguage.googleapis.com",
    "alkalimakersuite-pa.clients6.google.com",
    "alkalicore-pa.clients6.google.com",
    "webchannel-alkalimakersuite-pa.clients6.google.com",
    "robinfrontend-pa.googleapis.com",
    "webchannel-robinfrontend-pa.googleapis.com",
    "proactivebackend-pa.googleapis.com",
    "geller-pa.googleapis.com",
    "aicode.googleapis.com",
    "cloudcode-pa.googleapis.com",
    "daily-cloudcode-pa.googleapis.com",
    // OpenAI
    ".openai.com",
    ".chatgpt.com",
    ".oaistatic.com",
    ".oaiusercontent.com",
    ".sora.com",
    // Anthropic
    ".claude.ai",
    ".claude.com",
    ".anthropic.com",
    // xAI
    ".grok.com",
    ".x.ai",
    // Microsoft Copilot
    "copilot.microsoft.com",
];

struct Cached {
    bytes: Vec<u8>,
    until: Instant,
}

pub struct AiDns {
    task: Mutex<Option<JoinHandle<()>>>,
    /// Порядок опроса провайдеров (DoH URL). Первый — предпочтительный.
    order: Arc<Mutex<Vec<String>>>,
}

impl AiDns {
    pub fn new() -> Self {
        Self { task: Mutex::new(None), order: Arc::default() }
    }

    pub fn running(&self) -> bool {
        self.task.lock().unwrap().as_ref().is_some_and(|t| !t.is_finished())
    }

    /// `provider`: "auto" | id из [`PROVIDERS`].
    pub async fn start(&self, provider: &str, log: &Log) -> Result<()> {
        if cfg!(not(windows)) {
            bail!("разблокировка ИИ пока работает только в Windows");
        }
        self.set_provider(provider);
        if !self.running() {
            // PROSVET_AI_PORT — другой порт для диагностики (NRPT работает только с 53)
            let port = std::env::var("PROSVET_AI_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(53u16);
            let sock = UdpSocket::bind((LISTEN, port))
                .await
                .with_context(|| format!("не удалось занять {LISTEN}:{port} — порт занят другой программой"))?;
            let order = self.order.clone();
            let log2 = log.clone();
            *self.task.lock().unwrap() = Some(tokio::spawn(serve(sock, order, log2)));
            // Самопроверка: VPN-клиенты в режиме TUN (sing-box, Throne, Happ…) перехватывают
            // весь DNS на порт 53 — тогда наши запросы до сервера просто не доходят.
            if port == 53 && !self_test().await {
                self.stop();
                bail!("DNS-запросы перехватывает VPN-клиент (TUN-режим). С включённым VPN разблокировка не нужна — выключите её или VPN");
            }
        }
        apply_nrpt()?;
        log.push(format!("ИИ-сервисы: умный DNS включён ({})", self.order.lock().unwrap().first().map(|u| provider_name(u)).unwrap_or("?")));
        Ok(())
    }

    pub fn set_provider(&self, provider: &str) {
        let mut v: Vec<String> = PROVIDERS.iter().map(|p| p.2.to_string()).collect();
        if let Some(i) = PROVIDERS.iter().position(|p| p.0 == provider) {
            let first = v.remove(i);
            v.insert(0, first);
        }
        *self.order.lock().unwrap() = v;
    }

    pub fn stop(&self) {
        remove_nrpt();
        if let Some(t) = self.task.lock().unwrap().take() {
            t.abort();
        }
    }
}

fn provider_name(url: &str) -> &'static str {
    PROVIDERS.iter().find(|p| p.2 == url).map(|p| p.1).unwrap_or("?")
}

// ---------------------------------------------------------------- NRPT

fn ps(script: &str) -> Result<()> {
    let mut c = std::process::Command::new("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(crate::engine::CREATE_NO_WINDOW);
    }
    let out = c.output()?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Для «.domain» добавляем и сам домен, и все поддомены.
fn namespaces() -> Vec<String> {
    let mut v = Vec::new();
    for d in DOMAINS {
        if let Some(base) = d.strip_prefix('.') {
            v.push(base.to_string());
            v.push(format!(".{base}"));
        } else {
            v.push(d.to_string());
        }
    }
    v
}

fn apply_nrpt() -> Result<()> {
    let list = namespaces().iter().map(|n| format!("'{n}'")).collect::<Vec<_>>().join(",");
    ps(&format!(
        "Get-DnsClientNrptRule | Where-Object Comment -eq '{NRPT_COMMENT}' | Remove-DnsClientNrptRule -Force; \
         Add-DnsClientNrptRule -Namespace @({list}) -NameServers '{LISTEN}' -Comment '{NRPT_COMMENT}' | Out-Null; \
         Clear-DnsClientCache"
    ))
    .context("не удалось добавить правила NRPT")
}

/// Снимает наши правила (в том числе оставшиеся после аварийного завершения).
pub fn remove_nrpt() {
    if cfg!(not(windows)) {
        return;
    }
    let _ = ps(&format!(
        "Get-DnsClientNrptRule | Where-Object Comment -eq '{NRPT_COMMENT}' | Remove-DnsClientNrptRule -Force; Clear-DnsClientCache"
    ));
}

// ---------------------------------------------------------------- DNS-сервер

async fn serve(sock: UdpSocket, order: Arc<Mutex<Vec<String>>>, log: Log) {
    let sock = Arc::new(sock);
    // PROSVET_DOH_PROXY — пустить DoH через HTTP-прокси (диагностика, корпоративные сети)
    let mut b = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(3));
    if let Some(p) = std::env::var("PROSVET_DOH_PROXY").ok().and_then(|u| reqwest::Proxy::all(u).ok()) {
        b = b.proxy(p);
    }
    let client = b.build().expect("http client");
    let cache: Arc<Mutex<HashMap<(String, u16), Cached>>> = Arc::default();
    let warned = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(600)));
    let mut buf = vec![0u8; 1500];
    loop {
        let Ok((n, peer)) = sock.recv_from(&mut buf).await else {
            // на Windows сюда прилетает WSAECONNRESET после ICMP unreachable — не крутимся вхолостую
            tokio::time::sleep(Duration::from_millis(20)).await;
            continue;
        };
        let query = buf[..n].to_vec();
        let (sock, client, order, cache, log, warned) = (sock.clone(), client.clone(), order.clone(), cache.clone(), log.clone(), warned.clone());
        tokio::spawn(async move {
            let reply = answer(&query, &client, &order, &cache).await;
            match reply {
                Ok(r) => {
                    let _ = sock.send_to(&r, peer).await;
                }
                Err(err) => {
                    let warn = {
                        let mut w = warned.lock().unwrap();
                        let due = w.elapsed() > Duration::from_secs(300);
                        if due {
                            *w = Instant::now();
                        }
                        due
                    };
                    if warn {
                        log.push(format!("ИИ-сервисы: умные DNS не отвечают ({err})"));
                    }
                    if let Some(r) = empty_reply(&query, 2) {
                        let _ = sock.send_to(&r, peer).await;
                    }
                }
            }
        });
    }
}

async fn answer(
    query: &[u8],
    client: &reqwest::Client,
    order: &Mutex<Vec<String>>,
    cache: &Mutex<HashMap<(String, u16), Cached>>,
) -> Result<Vec<u8>, String> {
    let (name, qtype) = parse_question(query).ok_or("кривой запрос")?;
    // Только A: AAAA/HTTPS дали бы браузеру прямой адрес в обход прокси.
    if qtype != 1 {
        return empty_reply(query, 0).ok_or_else(|| "кривой запрос".into());
    }
    let key = (name, qtype);
    if let Some(c) = cache.lock().unwrap().get(&key) {
        if c.until > Instant::now() {
            let mut r = c.bytes.clone();
            r[..2].copy_from_slice(&query[..2]);
            return Ok(r);
        }
    }
    let mut wire = query.to_vec();
    wire[0] = 0;
    wire[1] = 0; // RFC 8484: id = 0 для кэшируемости
    let urls = order.lock().unwrap().clone();
    let mut last = String::from("нет провайдеров");
    for url in urls {
        let resp = client
            .post(&url)
            .header("content-type", "application/dns-message")
            .header("accept", "application/dns-message")
            .body(wire.clone())
            .send()
            .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                last = format!("{}: {e}", provider_name(&url));
                continue;
            }
        };
        if !resp.status().is_success() {
            last = format!("{}: HTTP {}", provider_name(&url), resp.status());
            continue;
        }
        let Ok(bytes) = resp.bytes().await else { continue };
        let mut r = bytes.to_vec();
        if r.len() < 12 {
            continue;
        }
        r[..2].copy_from_slice(&query[..2]);
        cache.lock().unwrap().insert(key, Cached { bytes: r.clone(), until: Instant::now() + Duration::from_secs(300) });
        return Ok(r);
    }
    Err(last)
}

async fn self_test() -> bool {
    let Ok(c) = UdpSocket::bind("127.0.0.1:0").await else { return false };
    // AAAA сервер отвечает сам, без сети
    let q = [0xab, 0xcd, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 4, b't', b'e', b's', b't', 7, b'p', b'r', b'o', b's', b'v', b'e', b't', 0, 0, 28, 0, 1];
    if c.send_to(&q, (LISTEN, 53)).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 512];
    matches!(tokio::time::timeout(Duration::from_secs(2), c.recv(&mut buf)).await, Ok(Ok(n)) if n >= 12 && buf[..2] == [0xab, 0xcd])
}

/// (имя в нижнем регистре, тип записи) из первого вопроса.
fn parse_question(q: &[u8]) -> Option<(String, u16)> {
    if q.len() < 12 || u16::from_be_bytes([q[4], q[5]]) == 0 {
        return None;
    }
    let mut i = 12;
    let mut labels = Vec::new();
    loop {
        let len = *q.get(i)? as usize;
        i += 1;
        if len == 0 {
            break;
        }
        labels.push(String::from_utf8_lossy(q.get(i..i + len)?).to_lowercase());
        i += len;
    }
    let qtype = u16::from_be_bytes([*q.get(i)?, *q.get(i + 1)?]);
    Some((labels.join("."), qtype))
}

/// Ответ без записей (rcode 0 — «нет данных», 2 — SERVFAIL).
fn empty_reply(q: &[u8], rcode: u8) -> Option<Vec<u8>> {
    let mut i = 12;
    loop {
        let len = *q.get(i)? as usize;
        i += 1;
        if len == 0 {
            break;
        }
        i += len;
    }
    let end = i + 4;
    if q.len() < end {
        return None;
    }
    let mut r = Vec::with_capacity(end);
    r.extend_from_slice(&q[..2]);
    r.push(0x81 | (q[2] & 0x01)); // QR, RD как в запросе
    r.push(0x80 | rcode); // RA
    r.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
    r.extend_from_slice(&q[12..end]);
    Some(r)
}

/// Проверка, что ИИ доступен: OpenAI отвечает 401 (нужен ключ), а не 403 «страна не поддерживается».
pub async fn probe() -> bool {
    let Ok(c) = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(8)).build() else { return false };
    match c.get("https://api.openai.com/v1/models").send().await {
        Ok(r) => r.status().as_u16() != 403,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for p in name.split('.') {
            q.push(p.len() as u8);
            q.extend_from_slice(p.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&[0, 1]);
        q
    }

    #[test]
    fn parses_and_blanks() {
        let q = query("Chatgpt.com", 28);
        assert_eq!(parse_question(&q), Some(("chatgpt.com".into(), 28)));
        let r = empty_reply(&q, 0).unwrap();
        assert_eq!(&r[..2], &[0x12, 0x34]);
        assert_eq!(r[3] & 0x0f, 0);
        assert_eq!(&r[6..8], &[0, 0]); // ancount = 0
        assert_eq!(r.len(), q.len());
    }

    #[test]
    fn namespaces_cover_root_and_subdomains() {
        let n = namespaces();
        assert!(n.contains(&"chatgpt.com".to_string()) && n.contains(&".chatgpt.com".to_string()));
        assert!(n.contains(&"gemini.google.com".to_string()));
        assert!(!n.iter().any(|x| x == ".google.com"));
    }

    /// Сервер отвечает на AAAA сам, без сети.
    #[tokio::test(flavor = "multi_thread")]
    async fn serves_blank_aaaa() {
        let sock = UdpSocket::bind(("127.0.0.153", 0)).await.unwrap();
        let addr = sock.local_addr().unwrap();
        let log = Log::new(None, |_| {});
        let h = tokio::spawn(serve(sock, Arc::default(), log));
        let c = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        c.send_to(&query("chatgpt.com", 28), addr).await.unwrap();
        let mut buf = [0u8; 512];
        let n = tokio::time::timeout(Duration::from_secs(3), c.recv(&mut buf)).await.expect("нет ответа").unwrap();
        assert_eq!(&buf[6..8], &[0, 0]);
        assert!(n > 12);
        h.abort();
    }
}
