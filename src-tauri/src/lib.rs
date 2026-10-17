mod aidns;
mod controller;
mod engine;
mod paths;
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
struct Info {
    version: &'static str,
    engines: updater::Versions,
    data_dir: String,
    defender_ok: bool,
}

#[tauri::command]
async fn get_info(c: Ctl<'_>) -> Res<Info> {
    let paths = c.paths.clone();
    tauri::async_runtime::spawn_blocking(move || Info {
        version: env!("CARGO_PKG_VERSION"),
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
