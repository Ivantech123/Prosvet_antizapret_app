use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Все рабочие пути приложения. Движки копируются из ресурсов в
/// `%LOCALAPPDATA%\Prosvet`, чтобы их можно было обновлять без переустановки.
#[derive(Clone, Debug)]
pub struct Paths {
    pub data: PathBuf,
    pub engine: PathBuf,
    pub z1: PathBuf,
    pub z2: PathBuf,
    pub lists: PathBuf,
    pub settings: PathBuf,
    pub state: PathBuf,
}

/// Пользовательские списки: создаются один раз и никогда не перезаписываются.
pub const USER_LISTS: &[(&str, &str)] = &[
    ("list-general-user.txt", "# Свои домены для обхода, по одному на строку\ndomain.example.abc\n"),
    ("list-exclude-user.txt", "# Домены, которые не нужно трогать\ndomain.example.abc\n"),
    ("ipset-exclude-user.txt", "203.0.113.113/32\n"),
];

pub const IPSET_NONE: &str = "203.0.113.113/32\n";

impl Paths {
    pub fn new() -> Result<Self> {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .context("LOCALAPPDATA не задан")?;
        let data = base.join("Prosvet");
        let engine = data.join("engine");
        Ok(Self {
            z1: engine.join("z1"),
            z2: engine.join("z2"),
            lists: data.join("lists"),
            settings: data.join("settings.json"),
            state: data.join("state.json"),
            engine,
            data,
        })
    }

    pub fn z1_exe(&self) -> PathBuf {
        self.z1.join("winws.exe")
    }

    pub fn z2_exe(&self) -> PathBuf {
        self.z2.join("winws2.exe")
    }

    pub fn version_file(&self) -> PathBuf {
        self.engine.join("version.json")
    }

    /// Разворачивает движки из ресурсов, если их нет или версия поменялась.
    /// Пользовательские списки и выбранный режим ipset сохраняются.
    pub fn install_from(&self, resources: &Path) -> Result<bool> {
        let res_ver = fs::read_to_string(resources.join("version.json")).unwrap_or_default();
        let cur_ver = fs::read_to_string(self.version_file()).unwrap_or_default();
        let engines_ok = self.z1_exe().exists() && self.z2_exe().exists();
        let fresh = !(engines_ok && !res_ver.is_empty() && res_ver == cur_ver);

        if fresh && resources.join("z1").exists() {
            fs::create_dir_all(&self.engine)?;
            copy_dir(&resources.join("z1"), &self.z1)?;
            copy_dir(&resources.join("z2"), &self.z2)?;
            fs::copy(resources.join("version.json"), self.version_file())?;
            self.install_lists(&resources.join("lists"))?;
        }
        self.ensure_user_lists()?;
        Ok(fresh)
    }

    /// Обновляет системные списки, не трогая пользовательские и ipset-all.txt.
    pub fn install_lists(&self, from: &Path) -> Result<()> {
        fs::create_dir_all(&self.lists)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let is_user = USER_LISTS.iter().any(|(n, _)| *n == name);
            let dst = self.lists.join(&name);
            if is_user || (name == "ipset-all.txt" && dst.exists()) {
                continue;
            }
            fs::copy(entry.path(), dst)?;
        }
        Ok(())
    }

    pub fn ensure_user_lists(&self) -> Result<()> {
        fs::create_dir_all(&self.lists)?;
        for (name, default) in USER_LISTS {
            let p = self.lists.join(name);
            if !p.exists() {
                fs::write(p, default)?;
            }
        }
        let ipset = self.lists.join("ipset-all.txt");
        if !ipset.exists() {
            fs::write(ipset, IPSET_NONE)?;
        }
        Ok(())
    }
}

pub fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from).with_context(|| format!("чтение {}", from.display()))? {
        let entry = entry?;
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dst)?;
        } else {
            fs::copy(entry.path(), &dst).with_context(|| format!("копирование {}", dst.display()))?;
        }
    }
    Ok(())
}

/// Путь для winws: Windows-путь с завершающим `\`.
pub fn dir_arg(p: &Path) -> String {
    let mut s = p.display().to_string();
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s
}
