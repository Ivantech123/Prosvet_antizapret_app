use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::controller::{Mode, Status};

const ICON_ON: &[u8] = include_bytes!("../icons/tray-on.png");
const ICON_OFF: &[u8] = include_bytes!("../icons/tray-off.png");
const ICON_BUSY: &[u8] = include_bytes!("../icons/tray-busy.png");

pub struct TrayItems {
    toggle: MenuItem<Wry>,
    update: MenuItem<Wry>,
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Открыть Prosvet", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Включить обход", true, None::<&str>)?;
    let reselect = MenuItem::with_id(app, "reselect", "Подобрать стратегию заново", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "Проверить доступ", true, None::<&str>)?;
    let pause15 = MenuItem::with_id(app, "pause15", "Пауза на 15 минут", true, None::<&str>)?;
    let pause60 = MenuItem::with_id(app, "pause60", "Пауза на час", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Проверить обновления", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &reselect,
            &check,
            &pause15,
            &pause60,
            &PredefinedMenuItem::separator(app)?,
            &update,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(ICON_OFF)?)
        .tooltip("Prosvet — выключен")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id.as_ref() {
            "show" => crate::show_main(app),
            "toggle" => crate::toggle(app),
            "reselect" => crate::reselect(app),
            "check" => crate::check_access(app),
            "pause15" => crate::pause(app, 15),
            "pause60" => crate::pause(app, 60),
            "update" => crate::app_update(app),
            "quit" => crate::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                crate::show_main(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayItems { toggle, update });
    Ok(())
}

pub fn refresh(app: &AppHandle, st: &Status) {
    let Some(tray) = app.tray_by_id("main") else { return };
    let (icon, mut tip, toggle) = match st.mode {
        Mode::On => (
            ICON_ON,
            format!("Prosvet — работает{}", st.strategy.as_ref().map(|s| format!("\n{}", s.name)).unwrap_or_default()),
            "Выключить обход",
        ),
        Mode::Starting | Mode::Testing | Mode::Updating => (ICON_BUSY, "Prosvet — подбор стратегии…".to_string(), "Остановить"),
        Mode::Error => (ICON_OFF, format!("Prosvet — ошибка\n{}", st.message.clone().unwrap_or_default()), "Включить обход"),
        Mode::Off if st.paused_until.is_some() => (ICON_OFF, "Prosvet — на паузе".to_string(), "Включить сейчас"),
        Mode::Off => (ICON_OFF, "Prosvet — выключен".to_string(), "Включить обход"),
    };
    let update = match &st.app_update {
        Some(u) => {
            tip.push_str(&format!("\nДоступна версия {}", u.version));
            match u.progress {
                Some(p) => format!("Загрузка обновления… {p}%"),
                None => format!("Установить Prosvet {}", u.version),
            }
        }
        None => "Проверить обновления".to_string(),
    };
    if let Ok(img) = Image::from_bytes(icon) {
        let _ = tray.set_icon(Some(img));
    }
    let _ = tray.set_tooltip(Some(tip));
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.toggle.set_text(toggle);
        let _ = items.update.set_text(update);
    }
}
