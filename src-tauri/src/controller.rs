//! Мозг приложения: включение, автоподбор стратегии, сторож.

use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::engine::{Engine, Log};
use crate::paths::{Paths, IPSET_NONE};
use crate::settings::{self, IpsetMode, Score, Settings, State};
use crate::strategy::{self, EngineKind, Strategy};
use crate::system::{self, Conflict};
use crate::tester::{self, Report, ServiceStatus};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Off,
    Starting,
    On,
    Testing,
    Updating,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct StrategyRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub done: u32,
    pub total: u32,
    pub name: String,
    pub best: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub mode: Mode,
    pub strategy: Option<StrategyRef>,
    pub services: Vec<ServiceStatus>,
    pub message: Option<String>,
    pub progress: Option<Progress>,
    pub checked_at: Option<u64>,
    pub conflicts: Vec<Conflict>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StrategyInfo {
    #[serde(flatten)]
    pub strategy: Strategy,
    pub score: Option<Score>,
    pub active: bool,
}

pub struct Controller {
    pub app: AppHandle,
    pub paths: Paths,
    pub log: Log,
    settings: Mutex<Settings>,
    state: Mutex<State>,
    engine: Mutex<Engine>,
    status: Mutex<Status>,
    op: tokio::sync::Mutex<()>,
    cancel: AtomicBool,
    watchdog_fails: AtomicU32,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn score_of(r: &Report) -> Score {
    Score { passed: r.passed, total: r.total, latency_ms: r.latency_ms, at: now() }
}

/// Лучше ли `a`, чем `b`: больше пройденных проверок, при равенстве — быстрее.
fn better(a: &Score, b: &Score) -> bool {
    a.passed > b.passed || (a.passed == b.passed && a.latency_ms < b.latency_ms)
}

impl Controller {
    pub fn new(app: AppHandle, paths: Paths) -> Arc<Self> {
        let emitter = app.clone();
        let _ = std::fs::create_dir_all(&paths.data);
        let log = Log::new(Some(&paths.data.join("prosvet.log")), move |line| {
            let _ = emitter.emit("log", line);
        });
        let settings: Settings = settings::load(&paths.settings);
        let state: State = settings::load(&paths.state);
        Arc::new(Self {
            app,
            log,
            settings: Mutex::new(settings),
            state: Mutex::new(state),
            engine: Mutex::new(Engine::new()),
            status: Mutex::new(Status::default()),
            op: tokio::sync::Mutex::new(()),
            cancel: AtomicBool::new(false),
            watchdog_fails: AtomicU32::new(0),
            paths,
        })
    }

    // ------------------------------------------------------------ состояние

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    fn update(&self, f: impl FnOnce(&mut Status)) {
        let snapshot = {
            let mut s = self.status.lock().unwrap();
            f(&mut s);
            s.clone()
        };
        crate::tray::refresh(&self.app, &snapshot);
        let _ = self.app.emit("status", &snapshot);
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    fn save_settings(&self) {
        settings::save(&self.paths.settings, &*self.settings.lock().unwrap());
    }

    fn remember(&self, id: &str, r: &Report) {
        let mut st = self.state.lock().unwrap();
        st.scores.insert(id.to_string(), score_of(r));
        settings::save(&self.paths.state, &*st);
    }

    pub fn strategies(&self) -> Vec<StrategyInfo> {
        let st = self.state.lock().unwrap();
        let active = self.engine.lock().unwrap().current.clone();
        strategy::load_all(&self.paths)
            .into_iter()
            .map(|s| StrategyInfo {
                score: st.scores.get(&s.id).cloned(),
                active: active.as_deref() == Some(s.id.as_str()),
                strategy: s,
            })
            .collect()
    }

    fn find(&self, id: &str) -> Option<Strategy> {
        strategy::load_all(&self.paths).into_iter().find(|s| s.id == id)
    }

    /// Порядок перебора: последняя рабочая, затем ранее успешные, затем остальные.
    fn candidates(&self) -> Vec<Strategy> {
        let set = self.settings();
        let st = self.state.lock().unwrap().clone();
        let mut all = strategy::load_all(&self.paths);
        // Игровой фильтр есть только в стратегиях Flowseal.
        if set.game_filter != settings::GameFilter::Off {
            all.sort_by_key(|s| s.engine != EngineKind::Z1);
        }
        let mut out = Vec::new();
        if let Some(id) = &set.strategy {
            if let Some(i) = all.iter().position(|s| &s.id == id) {
                out.push(all.remove(i));
            }
        }
        let (mut good, rest): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|s| st.scores.get(&s.id).is_some_and(|sc| sc.total > 0 && sc.passed == sc.total));
        good.sort_by(|a, b| {
            let (sa, sb) = (&st.scores[&a.id], &st.scores[&b.id]);
            sa.latency_ms.cmp(&sb.latency_ms)
        });
        out.extend(good);
        out.extend(rest);
        out
    }

    // ------------------------------------------------------------ движок

    fn exe(&self, kind: EngineKind) -> std::path::PathBuf {
        match kind {
            EngineKind::Z1 => self.paths.z1_exe(),
            EngineKind::Z2 => self.paths.z2_exe(),
        }
    }

    fn launch(&self, s: &Strategy) -> Result<()> {
        // Сначала гасим свой движок, иначе он сам попадёт в «конфликты».
        self.stop_engine();
        let conflicts = system::conflicts(None);
        if !conflicts.is_empty() {
            let names = conflicts.iter().map(|c| c.title.clone()).collect::<Vec<_>>().join(", ");
            self.update(|st| st.conflicts = conflicts);
            bail!("уже работает другой обход: {names}");
        }
        let args = s.expand(&self.paths, self.settings().game_filter);
        self.engine.lock().unwrap().start(&s.id, &self.exe(s.engine), &args, &self.log)?;
        Ok(())
    }

    /// Запуск с проверкой, что процесс не упал сразу (ошибка аргументов, драйвер).
    async fn launch_checked(&self, s: &Strategy) -> Result<()> {
        self.launch(s)?;
        tokio::time::sleep(Duration::from_millis(1300)).await;
        if !self.engine.lock().unwrap().alive() {
            let tail = self.log.tail(3).join(" | ");
            bail!("движок завершился: {tail}");
        }
        Ok(())
    }

    fn stop_engine(&self) {
        self.engine.lock().unwrap().stop();
    }

    // ------------------------------------------------------------ команды

    /// Главная кнопка. Пробует сохранённую стратегию, при неудаче — подбирает новую.
    pub async fn enable(self: &Arc<Self>) -> Result<()> {
        let _g = self.op.lock().await;
        system::ensure_tcp_timestamps();
        let saved = self.settings().strategy.and_then(|id| self.find(&id));
        let Some(s) = saved else {
            return self.auto_select_locked().await;
        };
        self.update(|st| {
            st.mode = Mode::Starting;
            st.message = Some("Запуск…".into());
            st.strategy = Some(StrategyRef { id: s.id.clone(), name: s.name.clone() });
        });
        if let Err(e) = self.launch_checked(&s).await {
            self.fail(e);
            return Ok(());
        }
        let r = tester::run().await;
        let prev = self.state.lock().unwrap().scores.get(&s.id).cloned();
        let ok = r.all_ok() || prev.is_some_and(|p| r.passed >= p.passed);
        if ok {
            self.remember(&s.id, &r);
            self.set_on(&s, &r, None);
            Ok(())
        } else {
            self.log.push(format!("Стратегия «{}» больше не справляется ({}/{}), подбираю новую", s.name, r.passed, r.total));
            self.auto_select_locked().await
        }
    }

    pub fn disable(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.stop_engine();
        self.log.push("■ обход выключен");
        self.update(|st| {
            st.mode = Mode::Off;
            st.message = None;
            st.progress = None;
        });
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub async fn auto_select(self: &Arc<Self>) -> Result<()> {
        let _g = self.op.lock().await;
        self.auto_select_locked().await
    }

    async fn auto_select_locked(self: &Arc<Self>) -> Result<()> {
        self.cancel.store(false, Ordering::SeqCst);
        system::ensure_tcp_timestamps();
        let fast = self.settings().fast_select;
        let list = self.candidates();
        let total = list.len() as u32;

        self.stop_engine();
        self.update(|st| {
            st.mode = Mode::Testing;
            st.message = Some("Проверяю, что заблокировано…".into());
            st.progress = Some(Progress { done: 0, total, name: "без обхода".into(), best: None });
        });
        let baseline = tester::run().await;
        self.log.push(format!("Без обхода доступно {}/{}", baseline.passed, baseline.total));
        self.log_failures(&baseline);
        self.update(|st| st.services = baseline.services.clone());

        let mut best: Option<(Strategy, Report)> = None;
        for (i, s) in list.into_iter().enumerate() {
            if self.cancel.load(Ordering::SeqCst) {
                break;
            }
            let best_name = best.as_ref().map(|(b, r)| format!("{} · {}/{}", b.name, r.passed, r.total));
            self.update(|st| {
                st.message = Some(format!("Пробую «{}»", s.name));
                st.progress = Some(Progress { done: i as u32, total, name: s.name.clone(), best: best_name });
            });
            if let Err(e) = self.launch_checked(&s).await {
                self.log.push(format!("✗ {}: {e}", s.name));
                if !self.status().conflicts.is_empty() {
                    break;
                }
                continue;
            }
            let r = tester::run().await;
            self.log.push(format!("{} {} — {}/{} за {} мс", if r.all_ok() { "✓" } else { "·" }, s.name, r.passed, r.total, r.latency_ms));
            self.remember(&s.id, &r);
            self.update(|st| st.services = r.services.clone());
            if !r.all_ok() {
                self.log_failures(&r);
            }
            let done = r.all_ok();
            if best.as_ref().map_or(true, |(_, b)| better(&score_of(&r), &score_of(b))) {
                best = Some((s, r));
            }
            if fast && done {
                break;
            }
        }
        self.stop_engine();

        let cancelled = self.cancel.swap(false, Ordering::SeqCst);
        if !self.status().conflicts.is_empty() {
            self.fail(anyhow!("мешает другой обход — остановите его и попробуйте снова"));
            return Ok(());
        }
        let Some((s, r)) = best else {
            if cancelled {
                self.update(|st| {
                    st.mode = Mode::Off;
                    st.message = None;
                    st.progress = None;
                });
            } else {
                self.fail(anyhow!("ни одна стратегия не запустилась — проверьте антивирус"));
            }
            return Ok(());
        };

        if let Err(e) = self.launch_checked(&s).await {
            self.fail(e);
            return Ok(());
        }
        {
            let mut set = self.settings.lock().unwrap();
            set.strategy = Some(s.id.clone());
        }
        self.save_settings();
        let note = if r.passed <= baseline.passed && !baseline.all_ok() {
            Some("Стратегии не улучшили доступ — возможно, сайты блокируют по IP".to_string())
        } else if baseline.all_ok() {
            Some("Блокировок не обнаружено — обход включён на всякий случай".to_string())
        } else {
            None
        };
        self.log.push(format!("★ выбрана «{}» ({}/{})", s.name, r.passed, r.total));
        self.set_on(&s, &r, note);
        Ok(())
    }

    /// Ручной выбор стратегии.
    pub async fn select(self: &Arc<Self>, id: &str) -> Result<()> {
        let _g = self.op.lock().await;
        let s = self.find(id).ok_or_else(|| anyhow!("стратегия не найдена"))?;
        self.update(|st| {
            st.mode = Mode::Starting;
            st.strategy = Some(StrategyRef { id: s.id.clone(), name: s.name.clone() });
            st.message = Some("Запуск…".into());
        });
        system::ensure_tcp_timestamps();
        if let Err(e) = self.launch_checked(&s).await {
            self.fail(e);
            return Ok(());
        }
        self.settings.lock().unwrap().strategy = Some(s.id.clone());
        self.save_settings();
        let r = tester::run().await;
        self.remember(&s.id, &r);
        self.set_on(&s, &r, None);
        Ok(())
    }

    /// Проверка доступности прямо сейчас (с текущей стратегией или без обхода).
    pub async fn check(self: &Arc<Self>) -> Result<Report> {
        let _g = self.op.lock().await;
        let r = tester::run().await;
        let id = self.engine.lock().unwrap().current.clone();
        if let Some(id) = id {
            self.remember(&id, &r);
        }
        self.update(|st| {
            st.services = r.services.clone();
            st.checked_at = Some(now());
        });
        Ok(r)
    }

    fn log_failures(&self, r: &Report) {
        for f in r.results.iter().filter(|x| !x.ok) {
            self.log.push(format!("    ✗ {} — {}", f.url, f.error.as_deref().unwrap_or("ошибка")));
        }
    }

    fn set_on(&self, s: &Strategy, r: &Report, note: Option<String>) {
        self.watchdog_fails.store(0, Ordering::SeqCst);
        self.update(|st| {
            st.mode = Mode::On;
            st.strategy = Some(StrategyRef { id: s.id.clone(), name: s.name.clone() });
            st.services = r.services.clone();
            st.message = note;
            st.progress = None;
            st.checked_at = Some(now());
            st.conflicts.clear();
        });
    }

    fn fail(&self, e: anyhow::Error) {
        self.stop_engine();
        self.log.push(format!("Ошибка: {e}"));
        self.update(|st| {
            st.mode = Mode::Error;
            st.message = Some(e.to_string());
            st.progress = None;
        });
    }

    // ------------------------------------------------------------ настройки

    pub async fn apply_settings(self: &Arc<Self>, mut new: Settings) -> Result<()> {
        let old = self.settings();
        // Стратегией управляет только бэкенд (автоподбор/ручной выбор).
        new.strategy = old.strategy.clone();
        if new.autostart != old.autostart {
            system::set_autostart(new.autostart)?;
        }
        if new.ipset != old.ipset {
            self.write_ipset(new.ipset)?;
        }
        let restart = new.game_filter != old.game_filter || new.ipset != old.ipset;
        *self.settings.lock().unwrap() = new;
        self.save_settings();
        if restart {
            self.restart_if_running().await?;
        }
        Ok(())
    }

    fn write_ipset(&self, mode: IpsetMode) -> Result<()> {
        let dst = self.paths.lists.join("ipset-all.txt");
        match mode {
            IpsetMode::None => fs::write(dst, IPSET_NONE)?,
            IpsetMode::Any => fs::write(dst, "")?,
            IpsetMode::Loaded => {
                fs::copy(self.paths.lists.join("ipset-all.txt.backup"), dst)?;
            }
        }
        Ok(())
    }

    /// Перезапуск движка с той же стратегией (после смены списков/настроек).
    pub async fn restart_if_running(self: &Arc<Self>) -> Result<()> {
        let _g = self.op.lock().await;
        let id = self.engine.lock().unwrap().current.clone();
        if let Some(s) = id.and_then(|id| self.find(&id)) {
            if let Err(e) = self.launch_checked(&s).await {
                self.fail(e);
            }
        }
        Ok(())
    }

    pub fn resolve_conflicts(&self) {
        let list = self.status().conflicts;
        system::resolve_conflicts(&list);
        self.log.push("Остановлены другие обходы");
        self.update(|st| {
            st.conflicts = system::conflicts(None);
            if st.mode == Mode::Error && st.conflicts.is_empty() {
                st.mode = Mode::Off;
                st.message = None;
            }
        });
    }

    pub fn detect_conflicts(&self) {
        if self.engine.lock().unwrap().current.is_some() {
            return;
        }
        let c = system::conflicts(None);
        self.update(|st| st.conflicts = c);
    }

    // ------------------------------------------------------------ обновление

    pub async fn update_engines(self: &Arc<Self>) -> Result<String> {
        let _g = self.op.lock().await;
        let cur = crate::updater::installed(&self.paths);
        let was = self.status();
        self.update(|st| {
            st.mode = Mode::Updating;
            st.message = Some("Скачиваю обновления…".into());
        });
        let result = async {
            let d = crate::updater::download().await?;
            if d.versions.flowseal == cur.flowseal && d.versions.zapret2 == cur.zapret2 {
                return Ok::<_, anyhow::Error>(format!("Уже последние версии: Flowseal {}, zapret2 {}", cur.flowseal, cur.zapret2));
            }
            let running = self.engine.lock().unwrap().current.clone();
            self.stop_engine();
            crate::updater::install(&self.paths, &d)?;
            if let Some(s) = running.and_then(|id| self.find(&id)) {
                self.launch_checked(&s).await?;
            }
            Ok(format!("Обновлено: Flowseal {}, zapret2 {}", d.versions.flowseal, d.versions.zapret2))
        }
        .await;
        match &result {
            Ok(msg) => {
                self.log.push(msg.clone());
                let running = self.engine.lock().unwrap().current.is_some();
                self.update(|st| {
                    st.mode = if running { Mode::On } else { was.mode.min_off() };
                    st.message = was.message.clone();
                });
            }
            Err(e) => self.fail(anyhow!("обновление не удалось: {e}")),
        }
        result
    }

    // ------------------------------------------------------------ сторож

    pub fn spawn_watchdog(self: &Arc<Self>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let interval = me.settings().watchdog_interval.max(60);
                tokio::time::sleep(Duration::from_secs(interval)).await;
                if !me.settings().watchdog || me.status().mode != Mode::On {
                    continue;
                }
                me.watchdog_tick().await;
            }
        });
    }

    async fn watchdog_tick(self: &Arc<Self>) {
        let Ok(guard) = self.op.try_lock() else { return };
        let id = self.engine.lock().unwrap().current.clone();
        let Some(s) = id.and_then(|id| self.find(&id)) else { return };

        if !self.engine.lock().unwrap().alive() {
            self.log.push("Сторож: движок остановился, перезапускаю");
            if let Err(e) = self.launch_checked(&s).await {
                self.fail(e);
                return;
            }
        }
        let r = tester::run().await;
        self.update(|st| {
            st.services = r.services.clone();
            st.checked_at = Some(now());
        });
        let reference = self.state.lock().unwrap().scores.get(&s.id).cloned();
        let degraded = reference.is_some_and(|p| r.passed + 2 <= p.passed) || r.services_ok() == 0;
        if !degraded {
            self.watchdog_fails.store(0, Ordering::SeqCst);
            return;
        }
        let n = self.watchdog_fails.fetch_add(1, Ordering::SeqCst) + 1;
        self.log.push(format!("Сторож: доступ ухудшился ({}/{}), попытка {n}", r.passed, r.total));
        if n >= 2 {
            drop(guard);
            self.log.push("Сторож: подбираю новую стратегию");
            let _ = self.auto_select().await;
        }
    }

    pub fn shutdown(&self) {
        self.stop_engine();
    }
}

impl Mode {
    fn min_off(self) -> Mode {
        match self {
            Mode::On | Mode::Starting | Mode::Testing | Mode::Updating => Mode::Off,
            m => m,
        }
    }
}
