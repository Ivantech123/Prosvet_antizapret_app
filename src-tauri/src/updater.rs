//! Обновление движков и стратегий из релизов GitHub:
//! Flowseal/zapret-discord-youtube (winws, стратегии, списки) и bol-van/zapret2 (winws2, lua).

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths::Paths;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Versions {
    pub flowseal: String,
    pub zapret2: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("Prosvet/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("http client")
}

async fn latest(c: &reqwest::Client, repo: &str) -> Result<Release> {
    c.get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .with_context(|| format!("релиз {repo}"))
}

pub fn installed(paths: &Paths) -> Versions {
    crate::settings::load(&paths.version_file())
}

/// Какие версии доступны (без скачивания архивов).
pub async fn available() -> Result<Versions> {
    let c = client();
    let (fs, z2) = tokio::try_join!(latest(&c, "Flowseal/zapret-discord-youtube"), latest(&c, "bol-van/zapret2"))?;
    Ok(Versions { flowseal: fs.tag_name, zapret2: z2.tag_name })
}

/// Скачивает архивы. Сама распаковка — в [`install`], её нужно вызывать
/// при остановленном движке (файлы winws заняты, пока он работает).
pub async fn download() -> Result<Downloaded> {
    let c = client();
    let (fs, z2) = tokio::try_join!(latest(&c, "Flowseal/zapret-discord-youtube"), latest(&c, "bol-van/zapret2"))?;
    let fs_url = &fs
        .assets
        .iter()
        .find(|a| a.name.ends_with(".zip"))
        .ok_or_else(|| anyhow!("в релизе Flowseal нет zip"))?
        .browser_download_url;
    let z2_url = &z2
        .assets
        .iter()
        .find(|a| a.name.starts_with("zapret2-v") && a.name.ends_with(".zip"))
        .ok_or_else(|| anyhow!("в релизе zapret2 нет zip"))?
        .browser_download_url;
    let get = |u: String| {
        let c = c.clone();
        async move { Ok::<_, anyhow::Error>(c.get(u).send().await?.error_for_status()?.bytes().await?.to_vec()) }
    };
    let (fs_zip, z2_zip) = tokio::try_join!(get(fs_url.clone()), get(z2_url.clone()))?;
    Ok(Downloaded { versions: Versions { flowseal: fs.tag_name, zapret2: z2.tag_name }, fs_zip, z2_zip })
}

pub struct Downloaded {
    pub versions: Versions,
    fs_zip: Vec<u8>,
    z2_zip: Vec<u8>,
}

pub fn install(paths: &Paths, d: &Downloaded) -> Result<()> {
    let staging = paths.data.join("update-tmp");
    let _ = fs::remove_dir_all(&staging);
    let lists_tmp = staging.join("lists");
    fs::create_dir_all(&lists_tmp)?;

    // Flowseal: bin/* → z1, general*.bat → z1/strategies, lists/* → lists
    let strategies = paths.z1.join("strategies");
    let _ = fs::remove_dir_all(&strategies);
    fs::create_dir_all(&strategies)?;
    extract(&d.fs_zip, |parts| {
        let name = parts.last()?.to_string();
        let lower = name.to_lowercase();
        if parts.len() >= 2 && parts[parts.len() - 2] == "bin" {
            Some(paths.z1.join(name))
        } else if parts.len() >= 2 && parts[parts.len() - 2] == "lists" {
            Some(lists_tmp.join(name))
        } else if lower.starts_with("general") && lower.ends_with(".bat") {
            Some(strategies.join(name))
        } else {
            None
        }
    })?;
    paths.install_lists(&lists_tmp)?;

    // zapret2: бинарники windows-x86_64, lua, фильтры windivert, фейки
    extract(&d.z2_zip, |parts| {
        let name = parts.last()?.to_string();
        let dir = parts[..parts.len() - 1].join("/");
        if dir.ends_with("binaries/windows-x86_64") {
            Some(paths.z2.join(name))
        } else if parts.len() == 3 && parts[1] == "lua" && name.ends_with(".lua") {
            Some(paths.z2.join("lua").join(name))
        } else if dir.ends_with("windivert.filter.examples") {
            Some(paths.z2.join("filter").join(name))
        } else if dir.ends_with("files/fake") {
            Some(paths.z2.join("fake").join(name))
        } else {
            None
        }
    })?;
    for e in fs::read_dir(&paths.z1)?.flatten() {
        if e.path().extension().is_some_and(|x| x == "bin") {
            let _ = fs::copy(e.path(), paths.z2.join("fake").join(e.file_name()));
        }
    }

    crate::settings::save(&paths.version_file(), &d.versions);
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

fn extract(zip: &[u8], mut route: impl FnMut(&[&str]) -> Option<PathBuf>) -> Result<()> {
    let mut a = zip::ZipArchive::new(Cursor::new(zip))?;
    for i in 0..a.len() {
        let mut f = a.by_index(i)?;
        if f.is_dir() {
            continue;
        }
        let Some(name) = f.enclosed_name() else { continue };
        let parts: Vec<String> = name.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        if let Some(dst) = route(&refs) {
            if let Some(dir) = dst.parent() {
                fs::create_dir_all(dir)?;
            }
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf)?;
            write_replace(&dst, &buf)?;
        }
    }
    Ok(())
}

/// Запись поверх существующего файла; если он занят — через переименование.
fn write_replace(dst: &Path, data: &[u8]) -> Result<()> {
    if fs::write(dst, data).is_ok() {
        return Ok(());
    }
    let old = dst.with_extension("old");
    let _ = fs::remove_file(&old);
    fs::rename(dst, &old).with_context(|| format!("файл занят: {}", dst.display()))?;
    fs::write(dst, data)?;
    Ok(())
}
