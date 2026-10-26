//! Linux: привилегированный помощник.
//!
//! nfqws и правила nftables требуют root, а графическое приложение от root
//! запускать нельзя. Поэтому Prosvet один раз за сеанс запускает сам себя через
//! `pkexec prosvet --helper` (Windows-аналог — манифест requireAdministrator).
//! Помощник читает команды из stdin построчно (JSON), отвечает в stdout и
//! пересылает туда же вывод движка. Закрылся stdin — Prosvet завершился или упал:
//! помощник гасит движок и снимает правила (аналог Job Object в Windows).

#![cfg_attr(not(unix), allow(dead_code))]

use serde::{Deserialize, Serialize};

#[cfg(unix)]
pub use imp::*;

/// Номер очереди NFQUEUE, в которую nftables отдаёт пакеты движку.
pub const QNUM: u16 = 200;
/// Метка пакетов, которые движок отправил сам, — их нельзя снова ставить в очередь.
const FWMARK: &str = "0x40000000";
const TABLE: &str = "inet prosvet";

/// Имена движков: помощник от root запускает только их.
const ENGINES: &[&str] = &["nfqws", "nfqws2"];
/// Чужие обходы на Linux: по ним ищем конфликты (system.rs).
pub const FOREIGN_SERVICES: &[&str] = &["zapret", "zapret2", "zapret-discord-youtube", "nfqws2@nfqws1"];
pub const FOREIGN_PROCESSES: &[&str] = &["nfqws", "nfqws2", "tpws", "tpws2"];

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
enum Req {
    Start { exe: String, args: Vec<String>, tcp: String, udp: String },
    Stop,
    Alive,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t", rename_all = "kebab-case")]
enum Msg {
    Log { line: String },
    Ok,
    Err { msg: String },
    Alive { alive: bool },
}

/// «443, 80,1024-65535,2053» → «80, 443, 1024-65535»: nftables не принимает
/// пересекающиеся интервалы в одном множестве, поэтому склеиваем их сами.
pub fn merge_ports(s: &str) -> String {
    let mut v: Vec<(u32, u32)> = s
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            let (a, b) = p.split_once('-').unwrap_or((p, p));
            let (a, b) = (a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?);
            (a >= 1 && a <= b && b <= 65535).then_some((a, b))
        })
        .collect();
    v.sort();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (a, b) in v {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged.iter().map(|&(a, b)| if a == b { a.to_string() } else { format!("{a}-{b}") }).collect::<Vec<_>>().join(", ")
}

/// Правила как у zapret: первые пакеты соединения — в очередь движку,
/// пакеты самого движка (с меткой) — мимо и без conntrack.
/// Аргументы движка как их запускает помощник. Общие с тестом dry-run.
///
/// После настройки движок сбрасывает права root (Lua от root он не выполняет).
/// `owner` — владелец файлов движка, то есть пользователь, запустивший Prosvet:
/// от его имени движок прочтёт списки и lua из домашней папки. Без него движок
/// уйдёт в nobody, а тот в домашнюю папку не пустят.
pub fn engine_args(args: &[String], owner: Option<(u32, u32)>) -> Vec<String> {
    // qnum — глобальный параметр, ставим до профилей --new
    let mut v = vec![format!("--qnum={QNUM}")];
    if let Some((uid, gid)) = owner {
        v.push(format!("--uid={uid}:{gid}"));
    }
    v.extend(args.iter().cloned());
    v
}

/// Владелец файла (uid, gid); root не годится — движок от root Lua не выполнит.
#[cfg(unix)]
pub fn owner_of(p: &std::path::Path) -> Option<(u32, u32)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(p).ok()?;
    (m.uid() != 0).then(|| (m.uid(), m.gid()))
}

pub fn ruleset(tcp: &str, udp: &str) -> String {
    let mut out = format!(
        "table {TABLE}\ndelete table {TABLE}\ntable {TABLE} {{\n\
         \tchain predefrag {{ type filter hook output priority -401; policy accept;\n\
         \t\tmeta mark & {FWMARK} != 0 notrack\n\t}}\n\
         \tchain post {{ type filter hook postrouting priority mangle; policy accept;\n\
         \t\tmeta mark & {FWMARK} != 0 return\n"
    );
    if !tcp.is_empty() {
        out += &format!("\t\ttcp dport {{ {tcp} }} ct original packets 1-12 queue num {QNUM} bypass\n");
    }
    if !udp.is_empty() {
        out += &format!("\t\tudp dport {{ {udp} }} ct original packets 1-12 queue num {QNUM} bypass\n");
    }
    out += "\t}\n";
    if !tcp.is_empty() {
        // ответы сервера нужны автоопределению TTL (autottl)
        out += &format!(
            "\tchain pre {{ type filter hook prerouting priority filter; policy accept;\n\
             \t\ttcp sport {{ {tcp} }} ct reply packets 1-3 queue num {QNUM} bypass\n\t}}\n"
        );
    }
    out += "}\n";
    out
}

#[cfg(unix)]
mod imp {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use anyhow::{anyhow, bail, Context, Result};

    use super::*;
    use crate::engine::Log;

    // ================================================================ сторона root

    fn send(out: &Mutex<std::io::Stdout>, m: &Msg) {
        if let Ok(s) = serde_json::to_string(m) {
            let mut o = out.lock().unwrap();
            let _ = writeln!(o, "{s}");
            let _ = o.flush();
        }
    }

    /// Точка входа `prosvet --helper` (запускается через pkexec).
    pub fn main() -> ! {
        let out = Arc::new(Mutex::new(std::io::stdout()));
        if unsafe { libc::geteuid() } != 0 {
            send(&out, &Msg::Err { msg: "помощник должен работать от root".into() });
            std::process::exit(1);
        }
        remove_rules();
        let mut child: Option<Child> = None;
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let reply = match serde_json::from_str::<Req>(&line) {
                Err(e) => Msg::Err { msg: format!("кривая команда: {e}") },
                Ok(Req::Start { exe, args, tcp, udp }) => {
                    stop(&mut child);
                    match start(Path::new(&exe), &args, &tcp, &udp, &out) {
                        Ok(c) => {
                            child = Some(c);
                            Msg::Ok
                        }
                        Err(e) => {
                            remove_rules();
                            Msg::Err { msg: format!("{e:#}") }
                        }
                    }
                }
                Ok(Req::Stop) => {
                    stop(&mut child);
                    Msg::Ok
                }
                Ok(Req::Alive) => Msg::Alive { alive: child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None))) },
            };
            send(&out, &reply);
        }
        stop(&mut child);
        std::process::exit(0);
    }

    fn stop(child: &mut Option<Child>) {
        if let Some(mut c) = child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        remove_rules();
    }

    fn start(exe: &Path, args: &[String], tcp: &str, udp: &str, out: &Arc<Mutex<std::io::Stdout>>) -> Result<Child> {
        let name = exe.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !exe.is_absolute() || !ENGINES.contains(&name) {
            bail!("помощник запускает только nfqws/nfqws2, а не {}", exe.display());
        }
        if !exe.is_file() {
            bail!("не найден {}", exe.display());
        }
        apply_rules(&merge_ports(tcp), &merge_ports(udp))?;

        let mut cmd = Command::new(exe);
        cmd.args(engine_args(args, owner_of(exe)));
        cmd.current_dir(exe.parent().unwrap_or(Path::new("/"))).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        unsafe {
            use std::os::unix::process::CommandExt;
            // Помощник убит — движок умирает вместе с ним.
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        let mut c = cmd.spawn().with_context(|| format!("запуск {}", exe.display()))?;
        if let Some(o) = c.stdout.take() {
            forward(o, out.clone());
        }
        if let Some(e) = c.stderr.take() {
            forward(e, out.clone());
        }
        Ok(c)
    }

    fn forward(r: impl Read + Send + 'static, out: Arc<Mutex<std::io::Stdout>>) {
        thread::spawn(move || {
            for line in BufReader::new(r).split(b'\n').map_while(Result::ok) {
                let s = String::from_utf8_lossy(&line).trim_end().to_string();
                if !s.is_empty() {
                    send(&out, &Msg::Log { line: s });
                }
            }
        });
    }

    fn nft(script: &str) -> Result<()> {
        let mut c = Command::new("nft")
            .args(["-f", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("не найден nft — установите пакет nftables")?;
        c.stdin.take().unwrap().write_all(script.as_bytes())?;
        let o = c.wait_with_output()?;
        if !o.status.success() {
            bail!("nftables: {}", String::from_utf8_lossy(&o.stderr).trim());
        }
        Ok(())
    }

    fn apply_rules(tcp: &str, udp: &str) -> Result<()> {
        nft(&ruleset(tcp, udp))
    }

    fn remove_rules() {
        let _ = nft(&format!("table {TABLE}\ndelete table {TABLE}\n"));
    }


    // ================================================================ сторона приложения

    /// Связь с запущенным помощником.
    pub struct Client {
        child: Child,
        stdin: ChildStdin,
        rx: Receiver<Msg>,
    }

    /// Что запускать от root: сам AppImage (его FUSE-папка root недоступна) или текущий бинарник.
    fn self_path() -> Result<PathBuf> {
        if let Some(p) = std::env::var_os("APPIMAGE") {
            return Ok(PathBuf::from(p));
        }
        Ok(std::env::current_exe()?)
    }

    impl Client {
        pub fn spawn(log: &Log) -> Result<Self> {
            let exe = self_path()?;
            let mut child = Command::new("pkexec")
                .arg(&exe)
                .arg("--helper")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .context("не найден pkexec — установите polkit")?;
            let stdin = child.stdin.take().context("нет stdin помощника")?;
            let stdout = child.stdout.take().context("нет stdout помощника")?;
            let (tx, rx) = mpsc::channel();
            let log = log.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    match serde_json::from_str::<Msg>(&line) {
                        Ok(Msg::Log { line }) => {
                            if !crate::engine::is_noise(&line) {
                                log.push(format!("» {line}"));
                            }
                        }
                        Ok(m) => {
                            if tx.send(m).is_err() {
                                break;
                            }
                        }
                        Err(_) => {}
                    }
                }
            });
            Ok(Self { child, stdin, rx })
        }

        pub fn running(&mut self) -> bool {
            matches!(self.child.try_wait(), Ok(None))
        }

        /// Первый запрос ждёт, пока пользователь введёт пароль, поэтому таймаут большой.
        fn request(&mut self, req: &Req, wait: Duration) -> Result<Msg> {
            writeln!(self.stdin, "{}", serde_json::to_string(req)?).map_err(|_| anyhow!("помощник не запущен — нужен пароль администратора"))?;
            self.stdin.flush()?;
            match self.rx.recv_timeout(wait) {
                Ok(Msg::Err { msg }) => bail!("{msg}"),
                Ok(m) => Ok(m),
                Err(RecvTimeoutError::Timeout) => bail!("помощник не отвечает"),
                Err(RecvTimeoutError::Disconnected) => bail!("нет прав администратора — пароль не введён"),
            }
        }

        pub fn start(&mut self, exe: &Path, args: Vec<String>, tcp: String, udp: String) -> Result<()> {
            let req = Req::Start { exe: exe.display().to_string(), args, tcp, udp };
            self.request(&req, Duration::from_secs(180)).map(|_| ())
        }

        pub fn stop(&mut self) -> Result<()> {
            self.request(&Req::Stop, Duration::from_secs(10)).map(|_| ())
        }

        pub fn alive(&mut self) -> bool {
            matches!(self.request(&Req::Alive, Duration::from_secs(5)), Ok(Msg::Alive { alive: true }))
        }

    }

    impl Drop for Client {
        fn drop(&mut self) {
            // stdin закроется вместе с self — помощник сам всё уберёт и выйдет
            let _ = self.child.try_wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_ports() {
        assert_eq!(merge_ports("443,80,1024-65535,2053,12"), "12, 80, 443, 1024-65535");
        assert_eq!(merge_ports("50000-50100, 19294-19344,443,50050-50200"), "443, 19294-19344, 50000-50200");
        assert_eq!(merge_ports("0,abc,70000"), "");
    }

    #[test]
    fn builds_ruleset() {
        let r = ruleset("80, 443", "");
        assert!(r.contains("tcp dport { 80, 443 } ct original packets 1-12 queue num 200 bypass"));
        assert!(!r.contains("udp dport"));
        assert!(r.starts_with("table inet prosvet\ndelete table inet prosvet\n"));
    }

    /// Ядро принимает правила (только проверка, ничего не применяется). Нужен root: в CI через sudo.
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn nft_ruleset_applies() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let r = ruleset(&merge_ports("80,443,2053,1024-65535"), &merge_ports("443,19294-19344,50000-50100"));
        let mut c = Command::new("sudo").args(["nft", "-c", "-f", "-"]).stdin(Stdio::piped()).spawn().unwrap();
        c.stdin.take().unwrap().write_all(r.as_bytes()).unwrap();
        assert!(c.wait().unwrap().success(), "nft отверг правила:\n{r}");
    }
}
