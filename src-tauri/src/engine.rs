//! Запуск и остановка winws / winws2.
//!
//! Процесс помещается в Job Object с KILL_ON_JOB_CLOSE: если Prosvet упадёт
//! или будет убит, движок завершится вместе с ним и не оставит «висящий» обход.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{bail, Context, Result};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const LOG_CAP: usize = 800;

/// Кольцевой журнал + колбэк для пересылки строк в интерфейс.
#[derive(Clone)]
pub struct Log {
    lines: Arc<Mutex<VecDeque<String>>>,
    sink: Arc<dyn Fn(&str) + Send + Sync>,
    file: Option<Arc<Mutex<std::fs::File>>>,
}

impl Log {
    /// `file` — журнал на диске (prosvet.log), обрезается при старте, если стал больше 1 МБ.
    pub fn new(file: Option<&Path>, sink: impl Fn(&str) + Send + Sync + 'static) -> Self {
        let file = file.and_then(|p| {
            if std::fs::metadata(p).map(|m| m.len() > 1 << 20).unwrap_or(false) {
                let _ = std::fs::remove_file(p);
            }
            std::fs::OpenOptions::new().create(true).append(true).open(p).ok()
        });
        Self { lines: Arc::default(), sink: Arc::new(sink), file: file.map(|f| Arc::new(Mutex::new(f))) }
    }

    pub fn push(&self, line: impl Into<String>) {
        let line = line.into();
        let stamped = format!("{} {}", clock(), line);
        {
            let mut l = self.lines.lock().unwrap();
            if l.len() >= LOG_CAP {
                l.pop_front();
            }
            l.push_back(stamped.clone());
        }
        if let Some(f) = &self.file {
            use std::io::Write;
            let _ = writeln!(f.lock().unwrap(), "{stamped}");
        }
        (self.sink)(&stamped);
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.lines.lock().unwrap().iter().cloned().collect()
    }

    /// Последние строки, пришедшие от движка (для текста ошибки).
    pub fn tail(&self, n: usize) -> Vec<String> {
        let l = self.lines.lock().unwrap();
        l.iter().rev().take(n).rev().cloned().collect()
    }
}

fn clock() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let local = secs as i64 + local_offset_secs();
    let s = local.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(windows)]
fn local_offset_secs() -> i64 {
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        _rest: [u8; 168],
    }
    extern "system" {
        fn GetTimeZoneInformation(tz: *mut TimeZoneInformation) -> u32;
    }
    let mut tz = TimeZoneInformation { bias: 0, _rest: [0; 168] };
    // 2 = TIME_ZONE_ID_DAYLIGHT; летнее время в РФ не используется, но учтём bias.
    unsafe { GetTimeZoneInformation(&mut tz) };
    -(tz.bias as i64) * 60
}

#[cfg(not(windows))]
fn local_offset_secs() -> i64 {
    0
}

pub struct Engine {
    child: Option<Child>,
    #[cfg(windows)]
    job: Option<job::Job>,
    pub current: Option<String>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            child: None,
            #[cfg(windows)]
            job: None,
            current: None,
        }
    }

    pub fn start(&mut self, id: &str, exe: &Path, args: &[String], log: &Log) -> Result<()> {
        self.stop();
        if !exe.exists() {
            bail!("не найден {} — возможно, его удалил антивирус", exe.display());
        }
        let cwd = exe.parent().context("нет папки движка")?;
        let mut cmd = Command::new(exe);
        cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let mut child = cmd.spawn().with_context(|| format!("запуск {}", exe.display()))?;
        #[cfg(windows)]
        {
            self.job = job::Job::new().ok();
            if let Some(j) = &self.job {
                j.assign(&child);
            }
        }
        if let Some(out) = child.stdout.take() {
            pipe(out, log.clone());
        }
        if let Some(err) = child.stderr.take() {
            pipe(err, log.clone());
        }
        log.push(format!("▶ {id}"));
        self.child = Some(child);
        self.current = Some(id.to_string());
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        #[cfg(windows)]
        {
            self.job = None;
        }
        self.current = None;
    }

    /// true, если процесс жив. Если умер — забывает его и возвращает false.
    pub fn alive(&mut self) -> bool {
        match self.child.as_mut().map(|c| c.try_wait()) {
            Some(Ok(None)) => true,
            Some(_) => {
                self.child = None;
                false
            }
            None => false,
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn pipe(r: impl Read + Send + 'static, log: Log) {
    thread::spawn(move || {
        let reader = BufReader::new(r);
        for line in reader.split(b'\n').map_while(Result::ok) {
            let s = String::from_utf8_lossy(&line).trim_end().to_string();
            if !s.is_empty() && !is_noise(&s) {
                log.push(s);
            }
        }
    });
}

/// Служебные строки движка, которые при каждом запуске одинаковы.
fn is_noise(line: &str) -> bool {
    const NOISE: &[&str] = &[
        "Loading hostlist", "Loading ipset", "loading plain text", "Loaded ", "we have ",
        "LUA v", "JIT: ", "github version", "windivert initialized",
    ];
    NOISE.iter().any(|n| line.starts_with(n))
}

/// Проверка аргументов движка без перехвата трафика.
#[cfg(test)]
pub fn dry_run(exe: &Path, args: &[String]) -> Result<()> {
    let cwd = exe.parent().context("нет папки движка")?;
    let mut cmd = Command::new(exe);
    cmd.arg("--dry-run").args(args).current_dir(cwd).stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output()?;
    if out.status.success() {
        Ok(())
    } else {
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        bail!("{}", text.lines().rev().take(3).collect::<Vec<_>>().join(" | "))
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct Job(HANDLE);
    // HANDLE — просто число ядра, им можно пользоваться из любого потока.
    unsafe impl Send for Job {}

    impl Job {
        pub fn new() -> Result<Self, ()> {
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return Err(());
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                Ok(Job(h))
            }
        }

        pub fn assign(&self, child: &Child) {
            unsafe {
                AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
