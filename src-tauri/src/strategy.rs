//! Стратегии обхода. Два источника:
//! * встроенные стратегии zapret2 (winws2, Lua) — собираются из шаблона;
//! * `.bat` от Flowseal (winws, zapret1) — разбираются на лету, поэтому
//!   обновление Flowseal подхватывается без пересборки приложения.
//!
//! В аргументах используются плейсхолдеры, которые раскрываются при запуске:
//! `{Z1}` `{Z2}` `{LISTS}` — папки с `\` на конце, `{GAME_TCP}` `{GAME_UDP}` — порты.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::paths::{dir_arg, Paths};
use crate::settings::GameFilter;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    Z1,
    Z2,
}

#[derive(Clone, Debug, Serialize)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    pub engine: EngineKind,
    /// Короткое описание техники для интерфейса.
    pub hint: String,
    #[serde(skip)]
    pub args: Vec<String>,
}

impl Strategy {
    pub fn expand(&self, paths: &Paths, game: GameFilter) -> Vec<String> {
        let (gt, gu) = game.ports();
        let z1 = dir_arg(&paths.z1);
        let z2 = dir_arg(&paths.z2);
        let lists = dir_arg(&paths.lists);
        self.args
            .iter()
            .map(|a| {
                // В шаблонах пути написаны по-windows (lua\\zapret-lib.lua)
                let a = if cfg!(windows) { a.clone() } else { a.replace('\\', "/") };
                a.replace("{Z1}", &z1)
                    .replace("{Z2}", &z2)
                    .replace("{LISTS}", &lists)
                    .replace("{GAME_TCP}", gt)
                    .replace("{GAME_UDP}", gu)
            })
            .collect()
    }
}

/// UDP-порты, которые на Windows ловят фильтры --wf-raw-part (голос Discord, STUN, QUIC).
const RAW_PART_UDP: &str = "443,3478-3481,5349,19294-19344,50000-50100";

/// Linux: аргументы WinDivert (`--wf-*`) движку не нужны — пакеты ему отдают
/// правила nftables. Вынимаем из них порты: (аргументы, порты TCP, порты UDP).
pub fn split_wf(args: &[String]) -> (Vec<String>, String, String) {
    let (mut tcp, mut udp, mut raw) = (Vec::new(), Vec::new(), false);
    let mut rest = Vec::new();
    for a in args {
        let Some(opt) = a.strip_prefix("--wf-") else {
            rest.push(a.clone());
            continue;
        };
        let (key, val) = opt.split_once('=').unwrap_or((opt, ""));
        match key {
            "tcp" | "tcp-out" => tcp.push(val.to_string()),
            "udp" | "udp-out" => udp.push(val.to_string()),
            "raw" | "raw-part" => raw = true,
            _ => {}
        }
    }
    if raw {
        udp.push(RAW_PART_UDP.into());
    }
    (rest, tcp.join(","), udp.join(","))
}

/// Все стратегии в порядке перебора: сначала zapret2, затем Flowseal.
pub fn load_all(paths: &Paths) -> Vec<Strategy> {
    let mut all = zapret2_builtin();
    all.extend(flowseal(&paths.z1.join("strategies")));
    all
}

// ---------------------------------------------------------------- zapret2

struct Z2Variant {
    id: &'static str,
    name: &'static str,
    hint: &'static str,
    tls: &'static [&'static str],
}

const Z2_VARIANTS: &[Z2Variant] = &[
    Z2Variant {
        id: "z2-seqovl-google",
        name: "Seqovl · Google",
        hint: "multisplit с перекрытием seq фейковым ClientHello google.com",
        tls: &["--lua-desync=multisplit:pos=1:seqovl=681:seqovl_pattern=tls_google"],
    },
    Z2Variant {
        id: "z2-seqovl-4pda",
        name: "Seqovl · 4pda",
        hint: "multisplit с перекрытием seq фейковым ClientHello 4pda.to",
        tls: &["--lua-desync=multisplit:pos=1:seqovl=568:seqovl_pattern=tls_4pda"],
    },
    Z2Variant {
        id: "z2-fake-disorder-md5",
        name: "Fake + disorder · MD5",
        hint: "фейк с подменой SNI и MD5-подписью, затем сегменты в обратном порядке",
        tls: &[
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid,sni=www.google.com",
            "--lua-desync=multidisorder:pos=1,midsld",
        ],
    },
    Z2Variant {
        id: "z2-fake-disorder-badseq",
        name: "Fake + disorder · badseq",
        hint: "фейк с неверным seq, разрез посередине домена",
        tls: &[
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:tcp_seq=-10000:repeats=6",
            "--lua-desync=multidisorder:pos=midsld",
        ],
    },
    Z2Variant {
        id: "z2-fake-split-seq",
        name: "Fake + split · seq",
        hint: "фейк fonts.google.com со сдвигом seq, multisplit по домену",
        tls: &[
            "--lua-desync=fake:blob=fake_default_tls:tcp_seq=10000:repeats=6:tls_mod=rnd,dupsid,sni=fonts.google.com",
            "--lua-desync=multisplit:pos=1,midsld:seqovl=1:seqovl_pattern=tls_google:tcp_ts_up",
        ],
    },
    Z2Variant {
        id: "z2-fake-split-ts",
        name: "Fake + split · timestamp",
        hint: "фейк с испорченным TCP timestamp, multisplit по домену",
        tls: &[
            "--lua-desync=fake:blob=tls_google:tcp_ts=-1000:repeats=6:tls_mod=rnd,dupsid",
            "--lua-desync=multisplit:pos=1,midsld",
        ],
    },
    Z2Variant {
        id: "z2-fakedsplit-autottl",
        name: "Fakedsplit · auto TTL",
        hint: "фейки с TTL, который не доходит до сервера",
        tls: &[
            "--lua-desync=fake:blob=fake_default_tls:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:repeats=4",
            "--lua-desync=fakedsplit:pos=1:ip_autottl=-2,3-20:ip6_autottl=-2,3-20",
        ],
    },
];

fn z2_lists_general() -> Vec<&'static str> {
    vec![
        "--hostlist={LISTS}list-general.txt",
        "--hostlist={LISTS}list-general-user.txt",
        "--hostlist-exclude={LISTS}list-exclude.txt",
        "--hostlist-exclude={LISTS}list-exclude-user.txt",
        "--ipset-exclude={LISTS}ipset-exclude.txt",
        "--ipset-exclude={LISTS}ipset-exclude-user.txt",
    ]
}

fn z2_lists_exclude() -> Vec<&'static str> {
    vec![
        "--hostlist-exclude={LISTS}list-exclude.txt",
        "--hostlist-exclude={LISTS}list-exclude-user.txt",
        "--ipset-exclude={LISTS}ipset-exclude.txt",
        "--ipset-exclude={LISTS}ipset-exclude-user.txt",
    ]
}

fn zapret2_builtin() -> Vec<Strategy> {
    Z2_VARIANTS
        .iter()
        .map(|v| {
            let mut a: Vec<&str> = vec![
                "--wf-tcp-out=80,443,2053,2083,2087,2096,8443",
                "--lua-init=@{Z2}lua\\zapret-lib.lua",
                "--lua-init=@{Z2}lua\\zapret-antidpi.lua",
                "--lua-init=fake_default_tls = tls_mod(fake_default_tls,'rnd,rndsni')",
                "--blob=quic_google:@{Z2}fake\\quic_initial_www_google_com.bin",
                "--blob=tls_google:@{Z2}fake\\tls_clienthello_www_google_com.bin",
                "--blob=tls_4pda:@{Z2}fake\\tls_clienthello_4pda_to.bin",
                "--blob=discord_udp:@{Z2}fake\\ACTIVE_DISCORD_UDP.bin",
                "--wf-raw-part=@{Z2}filter\\windivert_part.discord_media.txt",
                "--wf-raw-part=@{Z2}filter\\windivert_part.stun.txt",
                "--wf-raw-part=@{Z2}filter\\windivert_part.quic_initial_ietf.txt",
            ];
            // Discord: голосовые/медиа-серверы на нестандартных портах
            a.extend(["--filter-tcp=2053,2083,2087,2096,8443", "--hostlist-domains=discord.media", "--filter-l7=tls", "--out-range=-d10", "--payload=tls_client_hello"]);
            a.extend(v.tls);
            a.push("--new");
            // Google / YouTube
            a.extend(["--filter-tcp=443", "--filter-l7=tls", "--hostlist={LISTS}list-google.txt", "--out-range=-d10", "--payload=tls_client_hello"]);
            a.extend(v.tls);
            a.push("--new");
            // Общий список доменов
            a.extend(["--filter-tcp=443", "--filter-l7=tls"]);
            a.extend(z2_lists_general());
            a.extend(["--out-range=-d10", "--payload=tls_client_hello"]);
            a.extend(v.tls);
            a.push("--new");
            // HTTP
            a.extend(["--filter-tcp=80", "--filter-l7=http"]);
            a.extend(z2_lists_general());
            a.extend([
                "--out-range=-d10",
                "--payload=http_req",
                "--lua-desync=fake:blob=fake_default_http:tcp_md5",
                "--lua-desync=multisplit:pos=method+2",
                "--new",
            ]);
            // База IP (ipset)
            a.extend(["--filter-tcp=443,8443", "--filter-l7=tls", "--ipset={LISTS}ipset-all.txt"]);
            a.extend(z2_lists_exclude());
            a.extend(["--out-range=-d10", "--payload=tls_client_hello"]);
            a.extend(v.tls);
            a.push("--new");
            // QUIC (YouTube, Google) по спискам доменов
            a.extend(["--filter-udp=443", "--filter-l7=quic", "--hostlist={LISTS}list-google.txt"]);
            a.extend(z2_lists_general());
            a.extend(["--payload=quic_initial", "--lua-desync=fake:blob=quic_google:repeats=6", "--new"]);
            // QUIC по базе IP
            a.extend(["--filter-udp=443", "--filter-l7=quic", "--ipset={LISTS}ipset-all.txt"]);
            a.extend(z2_lists_exclude());
            a.extend(["--payload=quic_initial", "--lua-desync=fake:blob=quic_google:repeats=6", "--new"]);
            // Голос Discord
            a.extend([
                "--filter-l7=stun,discord",
                "--payload=stun,discord_ip_discovery",
                "--lua-desync=fake:blob=discord_udp:repeats=6",
            ]);

            Strategy {
                id: v.id.into(),
                name: v.name.into(),
                engine: EngineKind::Z2,
                hint: v.hint.into(),
                args: a.into_iter().map(String::from).collect(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------- Flowseal

fn flowseal(dir: &Path) -> Vec<Strategy> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bat")))
        .collect();
    files.sort_by_key(|p| natural_key(&p.file_stem().unwrap_or_default().to_string_lossy()));

    files
        .into_iter()
        .filter_map(|p| {
            let stem = p.file_stem()?.to_string_lossy().to_string();
            let text = fs::read_to_string(&p).ok()?;
            let args = parse_bat(&text)?;
            Some(Strategy {
                id: format!("fs:{stem}"),
                name: pretty_flowseal(&stem),
                engine: EngineKind::Z1,
                hint: describe_z1(&args),
                args,
            })
        })
        .collect()
}

/// `general (ALT10)` → (1, 10) — чтобы ALT10 шёл после ALT9, а `general` был первым.
fn natural_key(stem: &str) -> (u32, String, u32) {
    let inner = stem
        .strip_prefix("general")
        .unwrap_or(stem)
        .trim()
        .trim_matches(|c| c == '(' || c == ')')
        .to_string();
    if inner.is_empty() {
        return (0, String::new(), 0);
    }
    let digits: String = inner.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    let base = inner[..inner.len() - digits.len()].trim().to_string();
    (1, base, digits.parse().unwrap_or(1))
}

fn pretty_flowseal(stem: &str) -> String {
    let inner = stem
        .strip_prefix("general")
        .unwrap_or(stem)
        .trim()
        .trim_matches(|c| c == '(' || c == ')');
    if inner.is_empty() {
        "Flowseal · General".into()
    } else {
        let mut s = String::new();
        for (i, w) in inner.split_whitespace().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            let lower = w.to_lowercase();
            if lower.starts_with("alt") || lower == "exp" || lower == "tls" {
                s.push_str(&w.to_uppercase());
            } else {
                let mut c = lower.chars();
                if let Some(f) = c.next() {
                    s.extend(f.to_uppercase());
                    s.push_str(c.as_str());
                }
            }
        }
        format!("Flowseal · {s}")
    }
}

fn describe_z1(args: &[String]) -> String {
    let mut modes: Vec<String> = Vec::new();
    for a in args {
        if let Some(v) = a.strip_prefix("--dpi-desync=") {
            for m in v.split(',') {
                if !modes.iter().any(|x| x == m) {
                    modes.push(m.to_string());
                }
            }
        }
    }
    if modes.is_empty() {
        "winws".into()
    } else {
        format!("winws: {}", modes.join(", "))
    }
}

/// Достаёт аргументы winws из bat-файла Flowseal: склеивает строки с `^`,
/// разбивает с учётом кавычек и заменяет переменные на плейсхолдеры.
pub fn parse_bat(text: &str) -> Option<Vec<String>> {
    let mut joined = String::new();
    let mut started = false;
    for line in text.lines() {
        let line = line.trim_end();
        if !started {
            if let Some(i) = line.find("winws.exe\"") {
                started = true;
                joined.push_str(&line[i + "winws.exe\"".len()..]);
            } else {
                continue;
            }
        } else {
            joined.push_str(line);
        }
        if joined.ends_with('^') {
            joined.pop();
            joined.push(' ');
        } else {
            break;
        }
    }
    if !started {
        return None;
    }
    let s = joined
        .replace("%BIN%", "{Z1}")
        .replace("%LISTS%", "{LISTS}")
        .replace("%GameFilterTCP%", "{GAME_TCP}")
        .replace("%GameFilterUDP%", "{GAME_UDP}")
        .replace("%GameFilter%", "{GAME_TCP}");

    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut has = false;
    let mut escaped = false;
    for ch in s.chars() {
        // cmd: вне кавычек `^X` означает буквальный X (например `--dpi-desync-fake-tls=^!`).
        if escaped {
            cur.push(ch);
            has = true;
            escaped = false;
            continue;
        }
        match ch {
            '^' if !quoted => escaped = true,
            '"' => {
                quoted = !quoted;
                has = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        args.push(cur);
    }
    (!args.is_empty()).then_some(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flowseal_bat() {
        let bat = r#"@echo off
start "zapret: %~n0" /min "%BIN%winws.exe" --wf-tcp=80,443,%GameFilterTCP% ^
--filter-udp=443 --hostlist="%LISTS%list-general.txt" --dpi-desync=fake --new ^
--filter-tcp=443 --dpi-desync-fake-quic="%BIN%quic_initial_www_google_com.bin"
"#;
        let a = parse_bat(bat).unwrap();
        assert_eq!(a[0], "--wf-tcp=80,443,{GAME_TCP}");
        assert_eq!(a[2], "--hostlist={LISTS}list-general.txt");
        assert_eq!(a.last().unwrap(), "--dpi-desync-fake-quic={Z1}quic_initial_www_google_com.bin");
        assert_eq!(a.len(), 7);
    }

    #[test]
    fn splits_windivert_args() {
        let a: Vec<String> = ["--wf-tcp=80,443,{GAME_TCP}", "--wf-udp=443,19294-19344", "--filter-tcp=443", "--wf-raw-part=@x.txt", "--new"]
            .into_iter()
            .map(String::from)
            .collect();
        let (rest, tcp, udp) = split_wf(&a);
        assert_eq!(rest, vec!["--filter-tcp=443", "--new"]);
        assert_eq!(tcp, "80,443,{GAME_TCP}");
        assert_eq!(udp, format!("443,19294-19344,{RAW_PART_UDP}"));
    }

    #[test]
    fn orders_flowseal_naturally() {
        let mut v = vec!["general (ALT10)", "general (ALT2)", "general", "general (ALT)", "general (SIMPLE FAKE)"];
        v.sort_by_key(|s| natural_key(s));
        assert_eq!(v, vec!["general", "general (ALT)", "general (ALT2)", "general (ALT10)", "general (SIMPLE FAKE)"]);
        assert_eq!(pretty_flowseal("general (FAKE TLS AUTO ALT2)"), "Flowseal · Fake TLS Auto ALT2");
    }

    /// Все стратегии должны приниматься движками (winws --dry-run) на реальных бинарниках.
    #[test]
    fn all_strategies_pass_dry_run() {
        let res = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("engine");
        let probe = res.join("z1").join(if cfg!(windows) { "winws.exe" } else { "nfqws" });
        if !probe.exists() {
            eprintln!("нет движков, пропуск (scripts/fetch-engine.ps1 / fetch-engine.sh)");
            return;
        }
        let paths = Paths {
            data: res.clone(),
            engine: res.clone(),
            z1: res.join("z1"),
            z2: res.join("z2"),
            lists: res.join("lists"),
            settings: res.join("_s.json"),
            state: res.join("_st.json"),
            update: res.join("_update"),
        };
        for (n, name) in [("list-general-user.txt", "domain.example.abc"), ("list-exclude-user.txt", "domain.example.abc"), ("ipset-exclude-user.txt", "203.0.113.113/32")] {
            let p = paths.lists.join(n);
            if !p.exists() {
                std::fs::write(&p, name).unwrap();
            }
        }
        let mut failed = Vec::new();
        let all = load_all(&paths);
        assert!(all.len() >= 20, "стратегий слишком мало: {}", all.len());
        for s in &all {
            for game in [GameFilter::Off, GameFilter::All] {
                let exe = match s.engine {
                    EngineKind::Z1 => paths.z1_exe(),
                    EngineKind::Z2 => paths.z2_exe(),
                };
                let args = s.expand(&paths, game);
                // На Linux движку уходит то же, что от помощника: без --wf-*, с --qnum,
                // а от root — ещё и со сбросом прав до владельца файлов (--uid).
                #[cfg(unix)]
                let args = {
                    let owner = if unsafe { libc::geteuid() } == 0 { crate::helper::owner_of(&exe) } else { None };
                    crate::helper::engine_args(&split_wf(&args).0, owner)
                };
                if let Err(e) = crate::engine::dry_run(&exe, &args) {
                    failed.push(format!("{} ({game:?}): {e}", s.id));
                }
            }
        }
        for n in ["list-general-user.txt", "list-exclude-user.txt", "ipset-exclude-user.txt"] {
            let _ = std::fs::remove_file(paths.lists.join(n));
        }
        assert!(failed.is_empty(), "не прошли dry-run:
{}", failed.join("
"));
    }
}
