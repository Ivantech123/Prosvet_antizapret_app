use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameFilter {
    #[default]
    Off,
    Tcp,
    Udp,
    All,
}

impl GameFilter {
    /// Порты для подстановки вместо %GameFilterTCP% / %GameFilterUDP%.
    /// «12» — заглушка Flowseal: порт, который никто не использует.
    pub fn ports(self) -> (&'static str, &'static str) {
        const RANGE: &str = "1024-65535";
        match self {
            GameFilter::Off => ("12", "12"),
            GameFilter::Tcp => (RANGE, "12"),
            GameFilter::Udp => ("12", RANGE),
            GameFilter::All => (RANGE, RANGE),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IpsetMode {
    /// Только домены из списков.
    #[default]
    None,
    /// Плюс большая база IP-подсетей (Cloudflare, игры и т.п.).
    Loaded,
    /// Обрабатывать вообще все IP.
    Any,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Включать обход при запуске приложения.
    pub enable_on_launch: bool,
    /// Запуск вместе с Windows (через планировщик, с правами администратора).
    pub autostart: bool,
    /// Следить за работой и подбирать новую стратегию, если старая перестала работать.
    pub watchdog: bool,
    /// Интервал проверки сторожем, секунд.
    pub watchdog_interval: u64,
    /// Остановиться на первой стратегии, которая прошла все проверки.
    pub fast_select: bool,
    pub close_to_tray: bool,
    pub game_filter: GameFilter,
    pub ipset: IpsetMode,
    /// Выбранная стратегия (id). None — подобрать автоматически.
    pub strategy: Option<String>,
    /// system | light | dark
    pub theme: String,
    /// Пройдено ли приветствие при первом запуске.
    pub onboarded: bool,
    /// Разблокировка ИИ-сервисов через умный DNS.
    pub ai_unlock: bool,
    /// auto | comss | geohide
    pub ai_provider: String,
    /// Подбирать стратегию заново каждый день в `schedule_time`.
    pub schedule: bool,
    /// Время подбора по расписанию, «ЧЧ:ММ» по местному времени.
    pub schedule_time: String,
    /// Перед подбором по расписанию обновлять движки и стратегии.
    pub schedule_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enable_on_launch: true,
            autostart: false,
            watchdog: true,
            watchdog_interval: 180,
            fast_select: true,
            close_to_tray: true,
            game_filter: GameFilter::Off,
            ipset: IpsetMode::None,
            strategy: None,
            theme: "system".into(),
            onboarded: false,
            ai_unlock: false,
            ai_provider: "auto".into(),
            schedule: false,
            schedule_time: "03:00".into(),
            schedule_update: false,
        }
    }
}

/// Результат последней проверки стратегии.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Score {
    pub passed: u32,
    pub total: u32,
    pub latency_ms: u32,
    pub at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub scores: HashMap<String, Score>,
    /// День (номер от 1970-01-01, местное время) последнего подбора по расписанию.
    pub last_scheduled: Option<i64>,
}

pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

pub fn save<T: Serialize>(path: &Path, value: &T) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(value) {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, s).is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }
}
