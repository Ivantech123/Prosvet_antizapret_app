// В релизе не показываем консольное окно.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Linux: root-помощник, запущенный через pkexec (см. helper.rs) — без окна и Tauri.
    #[cfg(unix)]
    if std::env::args().any(|a| a == "--helper") {
        prosvet_lib::helper_main();
    }
    prosvet_lib::run()
}
