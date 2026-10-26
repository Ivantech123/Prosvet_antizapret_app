//! Взаимодействие с системой: конфликты с другими обходами, автозапуск,
//! исключения Defender, системные настройки TCP. Windows и Linux — свои реализации
//! с одинаковыми функциями.

use std::path::Path;
use std::process::{Command, Output};

use anyhow::{bail, Result};
use serde::Serialize;

#[cfg(windows)]
pub use win::*;

#[cfg(unix)]
pub use unix::*;

fn run(program: &str, args: &[&str]) -> std::io::Result<Output> {
    let mut c = Command::new(program);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(crate::engine::CREATE_NO_WINDOW);
    }
    c.output()
}

#[derive(Clone, Debug, Serialize)]
pub struct Conflict {
    pub kind: &'static str,
    pub id: String,
    pub title: String,
}

// ================================================================ Windows

#[cfg(windows)]
mod win {
    use super::*;

    const TASK_NAME: &str = "Prosvet";

    /// Службы других обходов, которые перехватывают тот же трафик.
    const FOREIGN_SERVICES: &[(&str, &str)] = &[
        ("zapret", "служба zapret (Flowseal)"),
        ("winws1", "служба winws1 (zapret-win-bundle)"),
        ("winws2", "служба winws2 (zapret-win-bundle)"),
        ("GoodbyeDPI", "служба GoodbyeDPI"),
    ];

    const FOREIGN_PROCESSES: &[&str] = &["winws.exe", "winws2.exe", "goodbyedpi.exe"];

    pub(crate) fn powershell(script: &str) -> Result<String> {
        let out = run("powershell", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])?;
        if !out.status.success() {
            bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn service_running(name: &str) -> bool {
        run("sc", &["query", name])
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
            .unwrap_or(false)
    }

    /// Ищет запущенные чужие обходы. `own_pid` — pid нашего движка, его пропускаем.
    pub fn conflicts(own_pid: Option<u32>) -> Vec<Conflict> {
        let mut v = Vec::new();
        for (name, title) in FOREIGN_SERVICES {
            if service_running(name) {
                v.push(Conflict { kind: "service", id: (*name).into(), title: (*title).into() });
            }
        }
        if let Ok(o) = run("tasklist", &["/FO", "CSV", "/NH"]) {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let cols: Vec<&str> = line.split("\",\"").map(|s| s.trim_matches('"')).collect();
                if cols.len() < 2 {
                    continue;
                }
                let (image, pid) = (cols[0].to_lowercase(), cols[1].parse::<u32>().ok());
                if FOREIGN_PROCESSES.contains(&image.as_str()) && pid != own_pid && !v.iter().any(|c| c.kind == "service") {
                    v.push(Conflict { kind: "process", id: image.clone(), title: format!("процесс {image}") });
                }
            }
        }
        v.dedup_by(|a, b| a.id == b.id);
        v
    }

    /// Останавливает чужие службы (не удаляя их) и завершает чужие процессы.
    pub fn resolve_conflicts(list: &[Conflict]) {
        for c in list {
            match c.kind {
                "service" => {
                    let _ = run("sc", &["stop", &c.id]);
                }
                _ => {
                    let _ = run("taskkill", &["/F", "/IM", &c.id]);
                }
            }
        }
    }

    /// Стратегии с fooling=ts требуют включённых TCP timestamps.
    pub fn ensure_tcp_timestamps() {
        let enabled = run("netsh", &["interface", "tcp", "show", "global"])
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.to_lowercase().contains("timestamps") && l.to_lowercase().contains("enabled"))
            })
            .unwrap_or(true);
        if !enabled {
            let _ = run("netsh", &["interface", "tcp", "set", "global", "timestamps=enabled"]);
        }
    }

    /// Задача планировщика «при входе в систему» с наивысшими правами —
    /// иначе Windows будет спрашивать UAC при каждом старте.
    pub fn set_autostart(enable: bool) -> Result<()> {
        if enable {
            let exe = std::env::current_exe()?;
            let tr = format!("\"{}\" --hidden", exe.display());
            let out = run("schtasks", &["/Create", "/TN", TASK_NAME, "/TR", &tr, "/SC", "ONLOGON", "/RL", "HIGHEST", "/F"])?;
            if !out.status.success() {
                bail!("schtasks: {}", String::from_utf8_lossy(&out.stderr).trim());
            }
            // По умолчанию задача не стартует от батареи — исправляем.
            let _ = powershell(&format!(
                "$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit 0; Set-ScheduledTask -TaskName '{TASK_NAME}' -Settings $s | Out-Null"
            ));
        } else {
            let _ = run("schtasks", &["/Delete", "/TN", TASK_NAME, "/F"]);
        }
        Ok(())
    }

    pub fn autostart_enabled() -> bool {
        run("schtasks", &["/Query", "/TN", TASK_NAME]).map(|o| o.status.success()).unwrap_or(false)
    }

    /// Defender считает winws «HackTool» и может удалить его. Добавляем исключение
    /// на папку с движками и на папку установки.
    pub fn add_defender_exclusion(paths: &[&Path]) -> Result<()> {
        let list = paths
            .iter()
            .map(|p| format!("'{}'", p.display().to_string().replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        powershell(&format!("Add-MpPreference -ExclusionPath {list}"))?;
        Ok(())
    }

    pub fn defender_excluded(path: &Path) -> bool {
        let p = path.display().to_string().to_lowercase();
        powershell("(Get-MpPreference).ExclusionPath -join \"`n\"")
            .map(|s| s.lines().any(|l| p.starts_with(&l.trim().to_lowercase()) && !l.trim().is_empty()))
            .unwrap_or(false)
    }

    /// Открывает проводник с выделенным файлом.
    pub fn reveal(p: &Path) {
        let _ = Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
    }

    pub fn open_folder(p: &Path) {
        let _ = Command::new("explorer").arg(p).spawn();
    }
}

// ================================================================ Linux

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::helper::{FOREIGN_PROCESSES, FOREIGN_SERVICES};

    /// Команда оболочки (для отчёта диагностики).
    pub(crate) fn shell(script: &str) -> Result<String> {
        let out = run("sh", &["-c", script])?;
        let mut s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !out.status.success() {
            s.push_str(String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(s)
    }

    pub fn conflicts(_own_pid: Option<u32>) -> Vec<Conflict> {
        let mut v = Vec::new();
        for name in FOREIGN_SERVICES {
            if run("systemctl", &["is-active", "--quiet", name]).is_ok_and(|o| o.status.success()) {
                v.push(Conflict { kind: "service", id: (*name).into(), title: format!("служба {name}") });
            }
        }
        if v.is_empty() {
            for name in FOREIGN_PROCESSES {
                if run("pgrep", &["-x", name]).is_ok_and(|o| o.status.success()) {
                    v.push(Conflict { kind: "process", id: (*name).into(), title: format!("процесс {name}") });
                }
            }
        }
        v
    }

    /// Чужие службы и процессы принадлежат root — останавливаем через pkexec.
    /// Имена берутся только из списков известных обходов, поэтому в оболочку
    /// не попадает ничего постороннего.
    pub fn resolve_conflicts(list: &[Conflict]) {
        let mut cmds = Vec::new();
        for c in list {
            if c.kind == "service" && FOREIGN_SERVICES.contains(&c.id.as_str()) {
                cmds.push(format!("systemctl stop '{}'", c.id));
            } else if FOREIGN_PROCESSES.contains(&c.id.as_str()) {
                cmds.push(format!("pkill -x '{}'", c.id));
            }
        }
        if !cmds.is_empty() {
            let _ = run("pkexec", &["sh", "-c", &cmds.join("; ")]);
        }
    }

    /// В Linux timestamps включены по умолчанию (net.ipv4.tcp_timestamps = 1).
    pub fn ensure_tcp_timestamps() {}

    fn autostart_file() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
        Some(base.join("autostart").join("prosvet.desktop"))
    }

    /// Автозапуск по стандарту freedesktop: ~/.config/autostart/prosvet.desktop.
    pub fn set_autostart(enable: bool) -> Result<()> {
        let Some(file) = autostart_file() else { bail!("не найдена домашняя папка") };
        if !enable {
            let _ = std::fs::remove_file(file);
            return Ok(());
        }
        // AppImage: запускать сам файл, а не временную папку, куда он смонтирован
        let exe = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).unwrap_or(std::env::current_exe()?);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(
            file,
            format!(
                "[Desktop Entry]\nType=Application\nName=Prosvet\nComment=Обход блокировок\nExec=\"{}\" --hidden\nIcon=prosvet\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
                exe.display()
            ),
        )?;
        Ok(())
    }

    pub fn autostart_enabled() -> bool {
        autostart_file().is_some_and(|f| f.exists())
    }

    pub fn add_defender_exclusion(_paths: &[&Path]) -> Result<()> {
        bail!("Microsoft Defender есть только в Windows")
    }

    pub fn defender_excluded(_path: &Path) -> bool {
        true
    }

    pub fn reveal(p: &Path) {
        if let Some(dir) = p.parent() {
            open_folder(dir);
        }
    }

    pub fn open_folder(p: &Path) {
        let _ = Command::new("xdg-open").arg(p).spawn();
    }
}
