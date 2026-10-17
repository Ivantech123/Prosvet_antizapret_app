// В релизе не показываем консольное окно.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    prosvet_lib::run()
}
