mod aidns;
mod appupdate;
mod controller;
mod diag;
mod engine;
mod helper;
mod paths;
mod schedule;
mod settings;
mod strategy;
mod system;
mod tester;
mod tray;
mod updater;

use std::fs;
use std::sync::Arc;

use serde::Serialize;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, RunEvent, State, WindowEvent};

use controller::{Controller, Mode, Status, StrategyInfo};
use settings::Settings;

type Ctl<'a> = State<'a, Arc<Controller>>;
type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Списки, которые пользователь может редактировать в интерфейсе.
const EDITABLE_LISTS: &[&str] = &["list-general-user.txt", "list-exclude-user.txt", "ipset-exclude-user.txt"];

// ------------------------------------------------------------------ команды

#[tauri::command]
fn get_status(c: Ctl) -> Status {
    c.status()
}

#[tauri::command]
fn get_settings(c: Ctl) -> Settings {
    let mut s = c.settings();
    s.autostart = system::autostart_enabled();
    s
}

#[tauri::command]
async fn set_settings(c: Ctl<'_>, settings: Settings) -> Res<()> {
    c.inner().clone().apply_settings(settings).await.map_err(err)
}

#[tauri::command]
fn get_strategies(c: Ctl) -> Vec<StrategyInfo> {
    c.strategies()
}

#[tauri::command]
async fn enable(c: Ctl<'_>) -> Res<()> {
    c.inner().clone().enable().await.map_err(err)
}

#[tauri::command]
fn disable(c: Ctl) {
    c.disable();
}

#[tauri::command]
fn cancel(c: Ctl) {
    c.cancel();
}

#[tauri::command]
async fn auto_select(c: Ctl<'_>) -> Res<()> {
    c.inner().clone().auto_select().await.map_err(err)
}

#[tauri::command]
async fn select_strategy(c: Ctl<'_>, id: String) -> Res<()> {
    c.inner().clone().select(&id).await.map_err(err)
}

#[tauri::command]
async fn check_now(c: Ctl<'_>) -> Res<tester::Report> {
    c.inner().clone().check().await.map_err(err)
}

#[tauri::command]
fn get_logs(c: Ctl) -> Vec<String> {
    c.log.snapshot()
}

#[tauri::command]
fn get_list(c: Ctl, name: String) -> Res<String> {
    if !EDITABLE_LISTS.contains(&name.as_str()) {
        return Err("неизвестный список".into());
    }
    fs::read_to_string(c.paths.lists.join(name)).map_err(err)
}

#[tauri::command]
async fn set_list(c: Ctl<'_>, name: String, content: String) -> Res<()> {
    if !EDITABLE_LISTS.contains(&name.as_str()) {
        return Err("неизвестный список".into());
    }
    let mut text: String = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    // Пустой список winws считает «всё подряд» — держим заглушку.
    if text.lines().all(|l| l.starts_with('#')) {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(if name.starts_with("ipset") { "203.0.113.113/32" } else { "domain.example.abc" });
    }
    text.push('\n');
    fs::write(c.paths.lists.join(&name), text).map_err(err)?;
    c.log.push(format!("Сохранён список {name}"));
    c.inner().clone().restart_if_running().await.map_err(err)
}

#[tauri::command]
fn resolve_conflicts(c: Ctl) {
    c.resolve_conflicts();
}

#[tauri::command]
async fn update_engines(c: Ctl<'_>) -> Res<String> {
    c.inner().clone().update_engines().await.map_err(err)
}

#[tauri::command]
async fn check_updates(c: Ctl<'_>) -> Res<(updater::Versions, updater::Versions)> {
    let latest = updater::available().await.map_err(err)?;
    Ok((updater::installed(&c.paths), latest))
}

#[derive(Serialize)]
struct SiteCheck {
    host: String,
    result: tester::TargetResult,
    /// В каком списке домен уже есть: «ваш список», «список Flowseal» или None.
    listed: Option<&'static str>,
}

/// «rutracker.org/forum» → ("https://rutracker.org/forum", "rutracker.org").
fn normalize_site(input: &str) -> Res<(String, String)> {
    let s = input.trim();
    if s.is_empty() {
        return Err("введите адрес сайта".into());
    }
    let full = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
    let u = reqwest::Url::parse(&full).map_err(|_| "непонятный адрес".to_string())?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err("нужен адрес сайта (http или https)".into());
    }
    let host = u.host_str().ok_or("в адресе нет домена")?.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    Ok((u.to_string(), host))
}

/// Домен или его родитель есть в списке.
fn in_list(path: &std::path::Path, host: &str) -> bool {
    fs::read_to_string(path).is_ok_and(|text| {
        text.lines().map(str::trim).any(|l| !l.is_empty() && !l.starts_with('#') && (host == l || host.ends_with(&format!(".{l}"))))
    })
}

fn listed(c: &Controller, host: &str) -> Option<&'static str> {
    if in_list(&c.paths.lists.join("list-general-user.txt"), host) {
        Some("ваш список")
    } else if in_list(&c.paths.lists.join("list-general.txt"), host) {
        Some("список Flowseal")
    } else {
        None
    }
}

#[tauri::command]
async fn check_site(c: Ctl<'_>, url: String) -> Res<SiteCheck> {
    let (url, host) = normalize_site(&url)?;
    let result = tester::check_url(&url).await;
    c.log.push(format!(
        "{} Проверка {host} — {}",
        if result.ok { "✓" } else { "✗" },
        result.error.as_deref().unwrap_or("открывается")
    ));
    Ok(SiteCheck { listed: listed(&c, &host), host, result })
}

/// Добавляет домен в «Обходить» и перезапускает движок.
#[tauri::command]
async fn add_site(c: Ctl<'_>, host: String) -> Res<()> {
    let host = host.trim().to_lowercase();
    let valid = host.contains('.') && host.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.');
    if !valid {
        return Err("это не похоже на домен".into());
    }
    let path = c.paths.lists.join("list-general-user.txt");
    if in_list(&path, &host) {
        return Ok(());
    }
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && *l != "domain.example.abc").collect();
    lines.push(&host);
    fs::write(&path, lines.join("\n") + "\n").map_err(err)?;
    c.log.push(format!("Добавлен в список: {host}"));
    c.inner().clone().restart_if_running().await.map_err(err)
}

#[tauri::command]
fn pause_bypass(c: Ctl, minutes: u64) {
    c.inner().pause(minutes.clamp(1, 24 * 60));
}

/// Собирает отчёт для баг-репорта и показывает его в проводнике.
#[tauri::command]
async fn export_diagnostics(c: Ctl<'_>) -> Res<String> {
    let c = c.inner().clone();
    let path = tauri::async_runtime::spawn_blocking(move || diag::export(&c)).await.map_err(err)?.map_err(err)?;
    system::reveal(&path);
    Ok(path.display().to_string())
}

#[tauri::command]
async fn check_app_update(c: Ctl<'_>) -> Res<Option<appupdate::AppUpdate>> {
    c.check_app_update().await.map_err(err)
}

#[tauri::command]
async fn install_app_update(app: AppHandle, c: Ctl<'_>) -> Res<()> {
    c.inner().clone().install_app_update().await.map_err(err)?;
    quit(&app);
    Ok(())
}

#[tauri::command]
fn dismiss_updated(c: Ctl) {
    c.dismiss_updated();
}

/// Точка входа root-помощника на Linux.
#[cfg(unix)]
pub fn helper_main() -> ! {
    helper::main()
}

#[derive(Serialize)]
struct Info {
    version: &'static str,
    /// windows | linux — интерфейс прячет то, чего в системе нет.
    os: &'static str,
    /// Репозиторий релизов Prosvet — для ссылки «что нового».
    repo: &'static str,
    engines: updater::Versions,
    data_dir: String,
    defender_ok: bool,
}

#[tauri::command]
async fn get_info(c: Ctl<'_>) -> Res<Info> {
    let paths = c.paths.clone();
    tauri::async_runtime::spawn_blocking(move || Info {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        repo: appupdate::REPO,
        engines: updater::installed(&paths),
        data_dir: paths.data.display().to_string(),
        defender_ok: system::defender_excluded(&paths.data),
    })
    .await
    .map_err(err)
}

#[tauri::command]
fn add_defender_exclusion(c: Ctl) -> Res<()> {
    let mut list = vec![c.paths.data.clone()];
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        list.push(dir);
    }
    let refs: Vec<&std::path::Path> = list.iter().map(|p| p.as_path()).collect();
    system::add_defender_exclusion(&refs).map_err(err)?;
    c.log.push("Добавлено исключение Microsoft Defender");
    Ok(())
}

#[tauri::command]
fn open_data_dir(c: Ctl) {
    system::open_folder(&c.paths.data);
}

#[tauri::command]
fn close_main(app: AppHandle, c: Ctl) {
    if c.settings().close_to_tray {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
    } else {
        quit(&app);
    }
}

// ------------------------------------------------------------------ трей / окно

pub(crate) fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub(crate) fn toggle(app: &AppHandle) {
    let c = app.state::<Arc<Controller>>().inner().clone();
    match c.status().mode {
        Mode::On | Mode::Starting | Mode::Testing => c.disable(),
        Mode::Updating => {}
        Mode::Off | Mode::Error => {
            tauri::async_runtime::spawn(async move {
                let _ = c.enable().await;
            });
        }
    }
}

pub(crate) fn reselect(app: &AppHandle) {
    let c = app.state::<Arc<Controller>>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = c.auto_select().await;
    });
}

/// Проверка доступа из трея.
pub(crate) fn check_access(app: &AppHandle) {
    let c = app.state::<Arc<Controller>>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = c.check().await;
    });
}

pub(crate) fn pause(app: &AppHandle, minutes: u64) {
    app.state::<Arc<Controller>>().inner().pause(minutes);
}

/// Пункт трея «Обновления»: есть новая версия — ставим, нет — проверяем.
pub(crate) fn app_update(app: &AppHandle) {
    let c = app.state::<Arc<Controller>>().inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if c.status().app_update.is_none() {
            match c.check_app_update().await {
                Ok(Some(_)) => show_main(&app),
                Ok(None) => c.log.push("Установлена последняя версия Prosvet"),
                Err(e) => c.log.push(format!("Проверка обновлений Prosvet не удалась: {e}")),
            }
        } else if c.install_app_update().await.is_ok() {
            quit(&app);
        }
    });
}

pub(crate) fn quit(app: &AppHandle) {
    app.state::<Arc<Controller>>().shutdown();
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let hidden = std::env::args().any(|a| a == "--hidden");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let paths = paths::Paths::new()?;
            let resources = app.path().resolve("resources/engine", BaseDirectory::Resource)?;
            match paths.install_from(&resources) {
                Ok(true) => eprintln!("движки развёрнуты в {}", paths.engine.display()),
                Ok(false) => {}
                Err(e) => eprintln!("не удалось развернуть движки: {e:#}"),
            }

            // правила NRPT могли остаться после аварийного завершения
            aidns::remove_nrpt();
            let c = Controller::new(handle.clone(), paths);
            app.manage(c.clone());
            tray::build(&handle)?;

            let onboarded = c.settings().onboarded;
            if !hidden || !onboarded {
                show_main(&handle);
            }
            c.detect_conflicts();
            // До прохождения приветствия обход не включаем — его запустит онбординг.
            if onboarded && c.settings().enable_on_launch && c.status().conflicts.is_empty() {
                let c2 = c.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = c2.enable().await;
                });
            }
            {
                let c3 = c.clone();
                tauri::async_runtime::spawn(async move { c3.apply_ai().await });
            }
            c.spawn_watchdog();
            c.spawn_scheduler();
            c.spawn_app_update_checker();
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let c = window.state::<Arc<Controller>>();
                if c.settings().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_settings,
            set_settings,
            get_strategies,
            enable,
            disable,
            cancel,
            auto_select,
            select_strategy,
            check_now,
            get_logs,
            get_list,
            set_list,
            resolve_conflicts,
            update_engines,
            check_updates,
            get_info,
            add_defender_exclusion,
            open_data_dir,
            close_main,
            check_app_update,
            install_app_update,
            dismiss_updated,
            check_site,
            add_site,
            pause_bypass,
            export_diagnostics,
        ])
        .build(tauri::generate_context!())
        .expect("ошибка запуска Prosvet");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(c) = app.try_state::<Arc<Controller>>() {
                c.shutdown();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_sites() {
        assert_eq!(normalize_site(" rutracker.org/forum ").unwrap(), ("https://rutracker.org/forum".into(), "rutracker.org".into()));
        assert_eq!(normalize_site("http://WWW.Example.com").unwrap().1, "example.com");
        assert!(normalize_site("").is_err());
        assert!(normalize_site("ftp://example.com").is_err());
    }

    #[test]
    fn finds_parent_domain_in_list() {
        let p = std::env::temp_dir().join("prosvet-test-list.txt");
        fs::write(&p, "# свои\nyoutube.com\n").unwrap();
        assert!(in_list(&p, "youtube.com") && in_list(&p, "m.youtube.com"));
        assert!(!in_list(&p, "notyoutube.com"));
        let _ = fs::remove_file(p);
    }
}
