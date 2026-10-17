//! Проверка доступности сервисов.
//!
//! ТСПУ часто не рвёт соединение, а «замораживает» его после ~16 КБ.
//! Поэтому для части адресов мы не просто ждём ответа, а дочитываем
//! заданное количество байт: если поток встал — проверка провалена.

use std::time::{Duration, Instant};

use futures::future::join_all;
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub service: &'static str,
    pub url: &'static str,
    /// Сколько байт тела нужно получить (0 — достаточно заголовков).
    pub min_bytes: usize,
}

/// Сервисы, отображаемые на главном экране (id, название).
pub const SERVICES: &[(&str, &str)] = &[
    ("youtube", "YouTube"),
    ("discord", "Discord"),
    ("google", "Google"),
    ("cloudflare", "Cloudflare"),
];

pub const TARGETS: &[Target] = &[
    Target { service: "youtube", url: "https://www.youtube.com/", min_bytes: 96 * 1024 },
    Target { service: "youtube", url: "https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg", min_bytes: 20 * 1024 },
    Target { service: "youtube", url: "https://redirector.googlevideo.com/report_mapping", min_bytes: 0 },
    Target { service: "discord", url: "https://discord.com/", min_bytes: 64 * 1024 },
    Target { service: "discord", url: "https://gateway.discord.gg/", min_bytes: 0 },
    Target { service: "discord", url: "https://cdn.discordapp.com/", min_bytes: 0 },
    Target { service: "discord", url: "https://updates.discord.com/", min_bytes: 0 },
    Target { service: "google", url: "https://www.google.com/", min_bytes: 32 * 1024 },
    Target { service: "google", url: "https://www.gstatic.com/generate_204", min_bytes: 0 },
    Target { service: "cloudflare", url: "https://www.cloudflare.com/", min_bytes: 64 * 1024 },
    Target { service: "cloudflare", url: "https://cdnjs.cloudflare.com/ajax/libs/jquery/3.7.1/jquery.min.js", min_bytes: 64 * 1024 },
];

#[derive(Clone, Debug, Serialize)]
pub struct TargetResult {
    pub service: String,
    pub url: String,
    pub ok: bool,
    pub ms: u32,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    pub ok: u32,
    pub total: u32,
    pub ms: u32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub results: Vec<TargetResult>,
    pub services: Vec<ServiceStatus>,
    pub passed: u32,
    pub total: u32,
    pub latency_ms: u32,
}

impl Report {
    pub fn all_ok(&self) -> bool {
        self.total > 0 && self.passed == self.total
    }

    /// Сколько сервисов работают полностью.
    pub fn services_ok(&self) -> usize {
        self.services.iter().filter(|s| s.total > 0 && s.ok == s.total).count()
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        // Каждое соединение — новое: старые соединения могли открыться до смены стратегии.
        .pool_max_idle_per_host(0)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(9))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0 Safari/537.36")
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .expect("http client")
}

async fn check(c: &reqwest::Client, t: Target) -> TargetResult {
    let start = Instant::now();
    let res: Result<(), String> = async {
        let mut resp = c.get(t.url).send().await.map_err(short_err)?;
        // Заморозка ТСПУ выглядит как таймаут посреди потока (ошибка из chunk()),
        // а штатно закончившийся короткий ответ — это успех.
        let mut got = 0usize;
        while got < t.min_bytes {
            match resp.chunk().await.map_err(|e| format!("{} после {} КБ", short_err(e), got / 1024))? {
                Some(b) => got += b.len(),
                None => break,
            }
        }
        Ok(())
    }
    .await;
    TargetResult {
        service: t.service.into(),
        url: t.url.into(),
        ok: res.is_ok(),
        ms: start.elapsed().as_millis() as u32,
        error: res.err(),
    }
}

fn short_err(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "таймаут".into()
    } else if e.is_connect() {
        "нет соединения".into()
    } else if e.is_body() || e.is_decode() {
        "обрыв при загрузке".into()
    } else {
        let mut s = e.to_string();
        s.truncate(80);
        s
    }
}

pub async fn run() -> Report {
    let c = client();
    let results = join_all(TARGETS.iter().map(|t| check(&c, *t))).await;

    let services = SERVICES
        .iter()
        .map(|(id, name)| {
            let mine: Vec<_> = results.iter().filter(|r| r.service == *id).collect();
            let ok: Vec<_> = mine.iter().filter(|r| r.ok).collect();
            ServiceStatus {
                id: (*id).into(),
                name: (*name).into(),
                ok: ok.len() as u32,
                total: mine.len() as u32,
                ms: if ok.is_empty() { 0 } else { ok.iter().map(|r| r.ms).sum::<u32>() / ok.len() as u32 },
            }
        })
        .collect();

    let passed = results.iter().filter(|r| r.ok).count() as u32;
    let latency_ms = results.iter().filter(|r| r.ok).map(|r| r.ms).sum();
    Report { total: results.len() as u32, passed, latency_ms, services, results }
}
