//! Самообновление Prosvet из релизов GitHub.
//!
//! * в последнем релизе [`REPO`] ищем установщик `*x64-setup.exe`;
//! * скачиваем его в `%LOCALAPPDATA%\Prosvet\update` и сверяем SHA-256 с тем,
//!   что публикует GitHub (поле `digest` у файла релиза или `<имя>.sha256` рядом).
//!   Без контрольной суммы не ставим: установщик запускается с правами администратора;
//! * запускаем установщик в пассивном режиме (только полоска прогресса) и выходим.
//!   Prosvet уже работает от администратора, поэтому UAC не спрашивается,
//!   а после установки фоновый PowerShell запускает новую версию.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use crate::engine::CREATE_NO_WINDOW;

/// Репозиторий с релизами Prosvet (owner/name).
pub const REPO: &str = "Ivantech123/Prosvet_antizapret_app";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    /// «sha256:…» — GitHub считает сам для загруженных файлов.
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppUpdate {
    pub version: String,
    /// Описание релиза (markdown как есть).
    pub notes: String,
    pub size: u64,
    /// Ход загрузки, %. None — ещё не качали.
    pub progress: Option<u8>,
    #[serde(skip)]
    url: String,
    #[serde(skip)]
    sha256: Option<String>,
    #[serde(skip)]
    sha256_url: Option<String>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("Prosvet/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build()
        .expect("http client")
}

/// «v1.2.3», «1.2», «1.2.3-beta» → [1, 2, 3]; суффиксы после «-»/«+» отбрасываются.
fn parse_version(s: &str) -> Vec<u64> {
    let core = s.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next().unwrap_or("");
    core.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Новее ли `tag`, чем `current`.
pub fn newer(tag: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse_version(tag), parse_version(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// Файл релиза для этой системы: установщик NSIS в Windows, AppImage в Linux.
fn is_installer(name: &str) -> bool {
    let n = name.to_lowercase();
    if cfg!(windows) {
        n.ends_with("-setup.exe") && n.contains("x64")
    } else {
        let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "amd64" };
        n.ends_with(".appimage") && n.contains(arch)
    }
}

/// Linux: обновить на месте можно только AppImage; пакеты .deb/.rpm обновляет система.
pub fn can_install() -> Result<()> {
    if cfg!(unix) && std::env::var_os("APPIMAGE").is_none() {
        bail!("Prosvet установлен пакетом — скачайте новую версию со страницы релизов");
    }
    Ok(())
}

fn parse_sha256(s: &str) -> Option<String> {
    let h = s.trim().strip_prefix("sha256:").unwrap_or(s.trim());
    let h = h.split_whitespace().next()?;
    (h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| h.to_lowercase())
}

/// Есть ли версия новее установленной. Ok(None) — уже последняя.
pub async fn check() -> Result<Option<AppUpdate>> {
    let r: Release = client()
        .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("релиз Prosvet")?;
    let version = r.tag_name.trim_start_matches(['v', 'V']).to_string();
    if !newer(&version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let a = r
        .assets
        .iter()
        .find(|a| is_installer(&a.name))
        .ok_or_else(|| anyhow!("в релизе {} нет установщика", r.tag_name))?;
    let sha_name = format!("{}.sha256", a.name).to_lowercase();
    Ok(Some(AppUpdate {
        version,
        notes: r.body.unwrap_or_default().trim().to_string(),
        size: a.size,
        progress: None,
        url: a.browser_download_url.clone(),
        sha256: a.digest.as_deref().and_then(parse_sha256),
        sha256_url: r.assets.iter().find(|x| x.name.to_lowercase() == sha_name).map(|x| x.browser_download_url.clone()),
    }))
}

/// Скачивает установщик в `dir` и проверяет контрольную сумму.
/// `progress` вызывается при каждом новом проценте.
pub async fn download(u: &AppUpdate, dir: &Path, progress: impl Fn(u8)) -> Result<PathBuf> {
    can_install()?;
    let c = client();
    let expected = match (&u.sha256, &u.sha256_url) {
        (Some(h), _) => h.clone(),
        (None, Some(url)) => {
            let text = c.get(url).send().await?.error_for_status()?.text().await?;
            parse_sha256(&text).ok_or_else(|| anyhow!("не разобрал файл контрольной суммы"))?
        }
        _ => bail!("в релизе нет контрольной суммы установщика — обновите вручную"),
    };

    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir)?;
    let part = dir.join("Prosvet-setup.exe.part");
    let dst = dir.join("Prosvet-setup.exe");

    let mut resp = c.get(&u.url).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(u.size).max(1);
    let mut file = fs::File::create(&part)?;
    let mut hasher = Sha256::new();
    let (mut got, mut last) = (0u64, 0u8);
    progress(0);
    while let Some(chunk) = resp.chunk().await.context("загрузка прервалась")? {
        hasher.update(&chunk);
        file.write_all(&chunk)?;
        got += chunk.len() as u64;
        let pct = (got * 100 / total).min(100) as u8;
        if pct != last {
            last = pct;
            progress(pct);
        }
    }
    file.sync_all()?;
    drop(file);

    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        let _ = fs::remove_file(&part);
        bail!("контрольная сумма не совпала — файл повреждён или подменён");
    }
    fs::rename(&part, &dst)?;
    Ok(dst)
}

/// Строка для PowerShell в одинарных кавычках.
#[cfg(windows)]
fn ps_quote(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "''"))
}

/// Запускает установку в фоне: пауза, пока Prosvet закроется, установщик `/P`
/// (пассивный режим NSIS от Tauri), затем запуск обновлённого приложения.
/// После успешного вызова приложение нужно сразу завершить.
#[cfg(windows)]
pub fn launch(installer: &Path) -> Result<()> {
    let exe = std::env::current_exe()?;
    let script = format!(
        "Start-Sleep -Seconds 3; \
         Start-Process -FilePath {} -ArgumentList '/P' -Wait; \
         Start-Process -FilePath {}",
        ps_quote(installer),
        ps_quote(&exe)
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]);
    #[cfg(windows)]
    {
        // Выходим из Job Object планировщика, если он есть, — иначе нас убьют вместе с Prosvet.
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().context("не удалось запустить установщик")?;
    Ok(())
}

/// Linux: подменяет файл AppImage новым (запущенный образ при этом не мешает)
/// и запускает обновлённый через пару секунд, когда текущий закроется.
#[cfg(unix)]
pub fn launch(installer: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let target = PathBuf::from(std::env::var_os("APPIMAGE").ok_or_else(|| anyhow!("Prosvet запущен не из AppImage"))?);
    let tmp = target.with_extension("new");
    fs::copy(installer, &tmp)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    fs::rename(&tmp, &target).with_context(|| format!("не удалось заменить {}", target.display()))?;
    Command::new("sh")
        .args(["-c", "sleep 2; exec \"$0\"", &target.display().to_string()])
        .spawn()
        .context("не удалось перезапустить Prosvet")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(newer("v0.4.0", "0.3.0"));
        assert!(newer("0.3.1", "0.3.0"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("v0.3.0", "0.3.0"));
        assert!(!newer("0.3", "0.3.0"));
        assert!(!newer("0.2.9", "0.3.0"));
        assert!(!newer("0.3.0-beta", "0.3.0"));
    }

    #[test]
    fn finds_installer_and_hash() {
        if cfg!(windows) {
            assert!(is_installer("Prosvet_0.4.0_x64-setup.exe"));
            assert!(is_installer("Prosvet_x64-setup.exe"));
            assert!(!is_installer("Prosvet_x64-setup.exe.sha256"));
            assert!(!is_installer("Prosvet_x64_ru-RU.msi"));
            assert!(!is_installer("Prosvet_0.4.0_amd64.AppImage"));
        } else if cfg!(target_arch = "x86_64") {
            assert!(is_installer("Prosvet_0.4.0_amd64.AppImage"));
            assert!(!is_installer("Prosvet_0.4.0_amd64.deb"));
            assert!(!is_installer("Prosvet_0.4.0_x64-setup.exe"));
        }
        let h = "a".repeat(64);
        assert_eq!(parse_sha256(&format!("sha256:{h}")), Some(h.clone()));
        assert_eq!(parse_sha256(&format!("{}  Prosvet_x64-setup.exe\n", h.to_uppercase())), Some(h));
        assert_eq!(parse_sha256("sha256:xyz"), None);
    }
}
