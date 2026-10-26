//! Отчёт для баг-репорта: zip с журналом, настройками, версиями и состоянием системы.
//! Личного в нём только пользовательские списки доменов.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use zip::write::SimpleFileOptions;

use crate::controller::Controller;
use crate::system;

/// Вывод PowerShell в UTF-8 (иначе русская Windows отдаст кракозябры в OEM-866).
#[cfg(windows)]
fn ps(script: &str) -> String {
    system::powershell(&format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8; {script}"))
        .unwrap_or_else(|e| format!("ошибка: {e}"))
}

#[cfg(windows)]
const OS_SCRIPT: &str = "$o = Get-CimInstance Win32_OperatingSystem; \"$($o.Caption) $($o.Version) $($o.OSArchitecture)\"";
#[cfg(windows)]
const NET_SCRIPT: &str = "'--- TCP'; Get-NetTCPSetting -SettingName Internet | Format-List Timestamps,AutoTuningLevelLocal | Out-String; \
     '--- DNS'; Get-DnsClientServerAddress -AddressFamily IPv4 | Where-Object ServerAddresses | Format-Table InterfaceAlias,ServerAddresses -AutoSize | Out-String -Width 200; \
     '--- NRPT'; Get-DnsClientNrptRule | Format-Table Namespace,NameServers,Comment -AutoSize | Out-String -Width 200";

#[cfg(unix)]
fn ps(script: &str) -> String {
    system::shell(script).unwrap_or_else(|e| format!("ошибка: {e}"))
}

#[cfg(unix)]
const OS_SCRIPT: &str = ". /etc/os-release 2>/dev/null; echo \"$PRETTY_NAME, ядро $(uname -r) $(uname -m)\"";
#[cfg(unix)]
const NET_SCRIPT: &str = "echo '--- TCP'; sysctl net.ipv4.tcp_timestamps 2>&1; \
     echo '--- адреса'; ip -br addr 2>&1; \
     echo '--- DNS'; (resolvectl status 2>/dev/null || cat /etc/resolv.conf) | head -n 40; \
     echo '--- nftables'; command -v nft; echo '--- pkexec'; command -v pkexec";

fn stamp() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        let mut t = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut t) };
        format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
    }
    #[cfg(unix)]
    {
        system::shell("date +%Y-%m-%d_%H-%M-%S").unwrap_or_else(|_| "now".into())
    }
}

pub fn export(c: &Controller) -> Result<PathBuf> {
    let p = &c.paths;
    let dir = p.data.join("diagnostics");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("prosvet-diag-{}.zip", stamp()));

    let status = c.status();
    let set = c.settings();
    let engines = crate::updater::installed(p);
    let conflicts = status.conflicts.iter().map(|x| x.title.as_str()).collect::<Vec<_>>().join(", ");
    let summary = format!(
        "Prosvet {}\n\
         Система: {}\n\
         Движки: zapret2 {} · Flowseal {}\n\
         Режим: {:?}\n\
         Стратегия: {}\n\
         Другие обходы: {}\n\
         ИИ-сервисы: {}\n\
         Автозапуск: {}\n",
        env!("CARGO_PKG_VERSION"),
        ps(OS_SCRIPT),
        engines.zapret2,
        engines.flowseal,
        status.mode,
        status.strategy.as_ref().map(|s| format!("{} ({})", s.name, s.id)).unwrap_or_else(|| "—".into()),
        if conflicts.is_empty() { "нет" } else { &conflicts },
        if set.ai_unlock { set.ai_provider.as_str() } else { "выключены" },
        system::autostart_enabled(),
    );

    let mut z = zip::ZipWriter::new(fs::File::create(&path)?);
    let opt = SimpleFileOptions::default();
    let mut add = |name: &str, data: &[u8]| -> Result<()> {
        z.start_file(name, opt)?;
        z.write_all(data)?;
        Ok(())
    };
    add("summary.txt", summary.as_bytes())?;
    add("status.json", serde_json::to_string_pretty(&status)?.as_bytes())?;
    add("log-session.txt", c.log.snapshot().join("\n").as_bytes())?;
    add("network.txt", ps(NET_SCRIPT).as_bytes())?;
    for (name, src) in [
        ("prosvet.log", p.data.join("prosvet.log")),
        ("settings.json", p.settings.clone()),
        ("state.json", p.state.clone()),
        ("engine-version.json", p.version_file()),
        ("lists/list-general-user.txt", p.lists.join("list-general-user.txt")),
        ("lists/list-exclude-user.txt", p.lists.join("list-exclude-user.txt")),
        ("lists/ipset-exclude-user.txt", p.lists.join("ipset-exclude-user.txt")),
    ] {
        if let Ok(data) = fs::read(&src) {
            add(name, &data)?;
        }
    }
    z.finish()?;
    c.log.push(format!("Сохранён отчёт {}", path.display()));
    Ok(path)
}
