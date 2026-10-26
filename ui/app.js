"use strict";

/* ════════════════════════════════════════════════════════════════
   Мост к бэкенду. Без Tauri (index.html открыт в браузере) работает
   мок — удобно править дизайн без сборки: index.html#dark&run
   ════════════════════════════════════════════════════════════════ */
const T = window.__TAURI__;
const api = T
  ? {
      invoke: (cmd, args) => T.core.invoke(cmd, args),
      listen: (ev, fn) => T.event.listen(ev, (e) => fn(e.payload)),
      minimize: () => T.window.getCurrentWindow().minimize(),
      open: (url) => T.opener.openUrl(url),
    }
  : mockApi();

const $ = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => [...root.querySelectorAll(s)];
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

const scene = new Scene($("#sky"));

let status = { mode: "off", services: [], conflicts: [] };
let settings = {};
let info = null;
let page = "home";
let appChecked = false; // ручная проверка уже сказала «последняя версия»
let appLater = null; // «Позже» — прячем баннер этой версии до перезапуска

/* Логотипы — Simple Icons (CC0). */
const SERVICES = {
  youtube: {
    name: "YouTube",
    color: "#ff3040",
    path: "M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.545 12 3.545 12 3.545s-7.505 0-9.377.505A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.505 9.376.505 9.376.505s7.505 0 9.377-.505a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z",
  },
  discord: {
    name: "Discord",
    color: "#6d7cff",
    path: "M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z",
  },
  google: {
    name: "Google",
    color: "#4f8bff",
    path: "M12.48 10.92v3.28h7.84c-.24 1.84-.853 3.187-1.787 4.133-1.147 1.147-2.933 2.4-6.053 2.4-4.827 0-8.6-3.893-8.6-8.72s3.773-8.72 8.6-8.72c2.6 0 4.507 1.027 5.907 2.347l2.307-2.307C18.747 1.44 16.133 0 12.48 0 5.867 0 .307 5.387.307 12s5.56 12 12.173 12c3.573 0 6.267-1.173 8.373-3.36 2.16-2.16 2.84-5.213 2.84-7.667 0-.76-.053-1.467-.173-2.053H12.48z",
  },
  cloudflare: {
    name: "Cloudflare",
    color: "#ff9233",
    path: "M16.5088 16.8447c.1475-.5068.0908-.9707-.1553-1.3154-.2246-.3164-.6045-.499-1.0615-.5205l-8.6592-.1123a.1559.1559 0 0 1-.1333-.0713c-.0283-.042-.0351-.0986-.021-.1553.0278-.084.1123-.1484.2036-.1562l8.7359-.1123c1.0351-.0489 2.1601-.8868 2.5537-1.9136l.499-1.3013c.0215-.0561.0293-.1128.0147-.168-.5625-2.5463-2.835-4.4453-5.5499-4.4453-2.5039 0-4.6284 1.6177-5.3876 3.8614-.4927-.3658-1.1187-.5625-1.794-.499-1.2026.119-2.1665 1.083-2.2861 2.2856-.0283.31-.0069.6128.0635.894C1.5683 13.171 0 14.7754 0 16.752c0 .1748.0142.3515.0352.5273.0141.083.0844.1475.1689.1475h15.9814c.0909 0 .1758-.0645.2032-.1553l.12-.4268zm2.7568-5.5634c-.0771 0-.1611 0-.2383.0112-.0566 0-.1054.0415-.127.0976l-.3378 1.1744c-.1475.5068-.0918.9707.1543 1.3164.2256.3164.6055.498 1.0625.5195l1.8437.1133c.0557 0 .1055.0263.1329.0703.0283.043.0351.1074.0214.1562-.0283.084-.1132.1485-.204.1553l-1.921.1123c-1.041.0488-2.1582.8867-2.5527 1.914l-.1406.3585c-.0283.0713.0215.1416.0986.1416h6.5977c.0771 0 .1474-.0489.169-.126.1122-.4082.1757-.837.1757-1.2803 0-2.6025-2.125-4.727-4.7344-4.727",
  },
};

const AI = { name: "ИИ", color: "#b98cff", path: "M12 1.5l2.6 7.1 7.4 2.9-7.4 2.9L12 21.5l-2.6-7.1L2 11.5l7.4-2.9z" };
const AI_STATE = { checking: ["", "Проверяю…"], ok: ["ok", "<b>Доступны</b>"], blocked: ["bad", "<b>Геоблок</b>"], error: ["bad", "<b>Ошибка</b>"] };

const WORD = { off: "Выключено", starting: "Запуск", on: "Работает", testing: "Ищу просвет", updating: "Обновляю", error: "Ошибка" };
const EYEBROW = { off: "Обход блокировок", starting: "Секунду", on: "Обход включён", testing: "Подбор стратегии", updating: "Движки", error: "Что-то пошло не так" };
const CTA = { off: "Включить", starting: "Запуск…", on: "Выключить", testing: "Остановить", updating: "Обновление…", error: "Попробовать снова" };
const busy = (m) => m === "starting" || m === "testing" || m === "updating";

/* ════════════════════════════════════════════════════════════════ */

function toast(text, ms = 2600) {
  const t = $("#toast");
  t.textContent = text;
  t.classList.add("show");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.remove("show"), ms);
}

async function call(cmd, args) {
  try {
    return await api.invoke(cmd, args);
  } catch (e) {
    toast(String(e), 4200);
    throw e;
  }
}

function applyTheme(theme) {
  if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
  scene.syncTheme();
}

const timeOf = (sec) => new Date(sec * 1000).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });

function syncScene() {
  if (onbStep >= 0 && onbStep < 3) return scene.set("intro");
  const m = status.mode;
  scene.set(busy(m) ? "testing" : m === "on" ? "on" : m === "error" ? "error" : "off");
}

/* ───────────── навигация ───────────── */

function moveInk() {
  const on = $(".tab.on");
  if (!on) return;
  const ink = $("#tab-ink");
  ink.style.left = `${on.offsetLeft}px`;
  ink.style.width = `${on.offsetWidth}px`;
}

function go(p) {
  page = p;
  document.body.dataset.page = p;
  $$(".tab").forEach((t) => t.classList.toggle("on", t.dataset.page === p));
  $$(".page").forEach((s) => s.classList.toggle("active", s.id === `page-${p}`));
  moveInk();
  scene.setInner(p !== "home");
  if (p === "strategies") loadStrategies();
  if (p === "lists") loadList();
  if (p === "settings") loadInfo().catch(() => {});
  if (p === "log") $("#log").scrollTop = $("#log").scrollHeight;
}

/* ───────────── главная ───────────── */

function svcState(s, short = false) {
  if (!s || !s.total) return ["", "Не проверено"];
  if (s.ok === s.total) return ["ok", short ? "<b>Работает</b>" : `<b>Работает</b> · ${s.ms} мс`];
  if (s.ok === 0) return ["bad", "<b>Недоступен</b>"];
  return ["warn", `<b>Частично</b> · ${s.ok}/${s.total}`];
}

function svcTile(m, cls, text, title = "") {
  return `<div class="svc ${cls}" title="${esc(title)}">
    <div class="svc-logo" style="color:${m.color};background:${m.color}22"><svg viewBox="0 0 24 24"><path d="${m.path}"/></svg></div>
    <div class="svc-text"><div class="svc-name">${m.name}</div><div class="svc-state">${text}</div></div>
  </div>`;
}

function renderServices() {
  const byId = Object.fromEntries((status.services || []).map((s) => [s.id, s]));
  const short = !!status.ai; // пять плиток — без задержки, она во всплывающей подсказке
  const tiles = Object.entries(SERVICES).map(([id, m]) => {
    const s = byId[id];
    return svcTile(m, ...svcState(s, short), s && s.ok ? `${s.ms} мс` : "");
  });
  if (status.ai) {
    const [cls, text] = AI_STATE[status.ai.state] || ["", ""];
    tiles.push(svcTile(AI, cls, text, status.ai.detail || "Gemini, ChatGPT, Claude через умный DNS"));
  }
  const dock = $("#services");
  dock.style.setProperty("--n", tiles.length);
  dock.innerHTML = tiles.join("");
}

function renderBanners() {
  const out = [];
  if (status.conflicts && status.conflicts.length) {
    const names = status.conflicts.map((c) => c.title).join(", ");
    out.push(`<div class="glass banner bad">
      <div class="banner-text"><b>Работает другой обход.</b> <span>${esc(names)} — вместе они мешают друг другу.</span></div>
      <button class="btn small" data-act="resolve">Остановить</button>
    </div>`);
  }
  if (status.ai && status.ai.state === "error") {
    out.push(`<div class="glass banner bad">
      <div class="banner-text"><b>ИИ-сервисы не включились.</b> <span>${esc(status.ai.detail || "")}</span></div>
      <button class="btn small" data-act="ai-off">Выключить</button>
    </div>`);
  }
  const u = status.app_update;
  if (u && appLater !== u.version) {
    const loading = u.progress != null;
    out.push(`<div class="glass banner gold">
      <div class="banner-text"><b>Доступен Prosvet ${esc(u.version)}.</b> <span>${loading ? `Скачиваю… ${u.progress}%` : "Установка займёт минуту, обход ненадолго прервётся."}</span></div>
      ${loading ? "" : `<button class="btn small" data-act="app-notes">Что нового</button><button class="btn small" data-act="app-later">Позже</button><button class="btn small primary" data-act="app-install">Обновить</button>`}
    </div>`);
  }
  if (status.updated_to) {
    out.push(`<div class="glass banner gold">
      <div class="banner-text"><b>Prosvet обновлён до ${esc(status.updated_to)}.</b> <span>Настройки и стратегии на месте.</span></div>
      <button class="btn small" data-act="app-notes">Что нового</button><button class="btn small" data-act="updated-ok">Хорошо</button>
    </div>`);
  }
  if (info && !info.defender_ok) {
    out.push(`<div class="glass banner">
      <div class="banner-text"><b>Defender может удалить движок.</b> <span>Добавьте исключение, чтобы обход не пропал.</span></div>
      <button class="btn small" data-act="defender">Добавить</button>
    </div>`);
  }
  // Тот же набор баннеров (например, меняется только процент загрузки) — без анимации появления.
  const box = $("#banners");
  const html = out.join("");
  if (html === box.innerHTML) return;
  const kinds = out.map((b) => b.slice(0, 80)).join("|");
  box.classList.toggle("still", kinds === box.dataset.kinds);
  box.dataset.kinds = kinds;
  box.innerHTML = html;
}

function renderStatus() {
  const m = status.mode;
  document.body.dataset.mode = m;
  syncScene();

  $("#eyebrow").textContent = EYEBROW[m] || "";
  $("#hero-title").textContent = WORD[m] || m;
  $("#cta-text").textContent = CTA[m] || "";

  let sub = "";
  if (m === "on") {
    sub = status.strategy ? `<span class="chip gold">${esc(status.strategy.name)}</span>` : "";
    if (status.message) sub += `<span>${esc(status.message)}</span>`;
    else if (status.checked_at) sub += `<span>проверено в ${timeOf(status.checked_at)}</span>`;
    if (settings.schedule && settings.schedule_time) sub += `<span class="next" title="Подбор по расписанию"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></svg>подбор в ${esc(settings.schedule_time)}</span>`;
  } else if (m === "off" && status.paused_until) sub = `Обход включится сам в ${timeOf(status.paused_until)}`;
  else if (m === "off") sub = "Нажмите — и пусть будет свет";
  else sub = esc(status.message || "");
  $("#hero-sub").innerHTML = sub;

  const p = status.progress;
  $("#progress").hidden = !p;
  if (p) {
    $("#progress-fill").style.width = `${Math.round((p.done / Math.max(p.total, 1)) * 100)}%`;
    $("#progress-text").textContent = `${p.done} / ${p.total} · ${p.name}${p.best ? ` — лучшая: ${p.best}` : ""}`;
  }
  $("#btn-cancel").hidden = m !== "testing";
  $("#btn-pause").hidden = m !== "on";
  if (m !== "on") $("#pause-opts").hidden = true;
  if (m === "off" && status.paused_until) {
    $("#eyebrow").textContent = "Пауза";
    $("#hero-title").textContent = "На паузе";
    $("#cta-text").textContent = "Включить сейчас";
  }
  $("#btn-reselect").hidden = m === "testing";
  $("#btn-check").disabled = busy(m);
  $("#btn-reselect").disabled = busy(m);

  renderServices();
  renderBanners();
  renderAppUpdate();
  onbStatus();
  if (page === "strategies") loadStrategies();
}

/* ───────────── стратегии ───────────── */

let stratFilter = "all";
let stratKey = "";

const SUN = '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="4" /><path d="M12 2.5v2M12 19.5v2M4.6 4.6 6 6M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4" /></svg>';
const full = (sc) => sc && sc.total && sc.passed === sc.total;

function scoreHtml(sc) {
  if (!sc || !sc.total) {
    return `<div class="score"><div class="score-top"><span>не проверялась</span></div><div class="pips none">${"<i></i>".repeat(11)}</div></div>`;
  }
  const pct = sc.passed / sc.total;
  const cls = pct === 1 ? "ok" : pct >= 0.5 ? "warn" : "bad";
  const pips = Array.from({ length: sc.total }, (_, i) => `<i class="${i < sc.passed ? "on" : ""}"></i>`).join("");
  return `<div class="score">
    <div class="score-top"><b>${sc.passed}<small>/${sc.total}</small></b><span>${(sc.latency_ms / 1000).toFixed(1)} с</span></div>
    <div class="pips ${cls}">${pips}</div>
  </div>`;
}

async function loadStrategies() {
  const list = await api.invoke("get_strategies");
  // лучшая — больше всех проверок, при равенстве быстрее
  const best = list
    .filter((s) => s.score && s.score.total && s.score.passed)
    .sort((a, b) => b.score.passed / b.score.total - a.score.passed / a.score.total || a.score.latency_ms - b.score.latency_ms)[0];
  const tested = list.filter((s) => s.score && s.score.total).length;
  const working = list.filter((s) => full(s.score)).length;
  $("#strat-stats").innerHTML =
    `<span class="stat"><b>${list.length}</b>всего</span>` +
    (tested ? `<span class="stat"><b>${tested}</b>проверено</span>` : "") +
    (working ? `<span class="stat ok"><b>${working}</b>работают</span>` : "");

  const filtered = list.filter((s) => {
    if (stratFilter === "z1" || stratFilter === "z2") return s.engine === stratFilter;
    if (stratFilter === "ok") return full(s.score);
    return true;
  });
  const html =
    filtered
      .map((s, i) => {
        const top = best && s.id === best.id;
        const n = list.indexOf(s) + 1;
        return `<div class="glass strat ${s.active ? "active" : ""} ${top ? "top" : ""}" style="--i:${Math.min(i, 12)}">
          <div class="rank" title="${s.active ? "Сейчас работает" : top ? "Лучшая по проверкам" : ""}">${s.active ? SUN : String(n).padStart(2, "0")}</div>
          <div style="min-width:0">
            <div class="strat-name">${esc(s.name)} <span class="tag ${s.engine}">${s.engine === "z2" ? "zapret2" : "zapret"}</span>${top && !s.active ? '<span class="tag z2">лучшая</span>' : ""}</div>
            <div class="strat-hint" title="${esc(s.hint)}">${esc(s.hint)}</div>
          </div>
          ${scoreHtml(s.score)}
          <button class="btn small" data-run="${esc(s.id)}" ${s.active || busy(status.mode) ? "disabled" : ""}>${s.active ? "Работает" : "Включить"}</button>
        </div>`;
      })
      .join("") || `<p class="sub">Здесь пока пусто.</p>`;
  // анимация появления — только когда список действительно сменился, а не на каждый статус
  const key = stratFilter + "|" + filtered.map((s) => s.id).join();
  const el = $("#strategy-list");
  el.classList.toggle("quiet", key === stratKey);
  stratKey = key;
  el.innerHTML = html;
}

/* ───────────── списки ───────────── */

const LIST_HELP = {
  "list-general-user.txt": "Домены, к которым применять обход. Например: rutracker.org",
  "list-exclude-user.txt": "Домены, которые обход трогать не должен — если какой-то сайт стал работать хуже.",
  "ipset-exclude-user.txt": "IP-адреса или подсети-исключения. Например: 192.168.0.0/16",
};
let currentList = "list-general-user.txt";

async function loadList() {
  $$("#list-tabs button").forEach((b) => b.classList.toggle("on", b.dataset.list === currentList));
  $("#list-help").textContent = LIST_HELP[currentList];
  $("#list-editor").value = await api.invoke("get_list", { name: currentList });
  $("#list-saved").textContent = "";
  listChanged();
}

const plural = (n, one, few, many) => (n % 10 === 1 && n % 100 !== 11 ? one : n % 10 >= 2 && n % 10 <= 4 && (n % 100 < 10 || n % 100 >= 20) ? few : many);

/** Номера строк, счётчик записей и высота поля под текст (прокручивается вся панель). */
function listChanged() {
  const ta = $("#list-editor");
  const lines = ta.value.split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  $("#list-gutter").textContent = lines.map((_, i) => i + 1).join("\n");
  const n = lines.filter((l) => l.trim() && !l.trim().startsWith("#")).length;
  const ip = currentList.startsWith("ipset");
  $("#list-count").textContent = n ? `${n} ${ip ? plural(n, "адрес", "адреса", "адресов") : plural(n, "домен", "домена", "доменов")}` : "пусто";
  ta.style.height = "auto";
  ta.style.height = `${ta.scrollHeight}px`;
}

async function saveList() {
  await call("set_list", { name: currentList, content: $("#list-editor").value });
  await loadList();
  $("#list-saved").textContent = "Сохранено ✓";
}

/* ───────────── проверка сайта ───────────── */

async function checkSite() {
  const url = $("#site-input").value.trim();
  if (!url) return $("#site-input").focus();
  const box = $("#site-result");
  $("#btn-site").disabled = true;
  box.hidden = false;
  box.className = "site-result";
  box.innerHTML = `<div class="msg"><span>Проверяю ${esc(url)}…</span></div>`;
  try {
    const r = await api.invoke("check_site", { url });
    const ok = r.result.ok;
    const sec = (r.result.ms / 1000).toFixed(1);
    const where = r.listed ? `уже в «${esc(r.listed)}»` : "";
    let hint = "";
    if (!ok && !r.listed) hint = status.mode === "on" ? "Добавьте его в список — обход перезапустится с ним" : "Включите обход и добавьте сайт в список";
    else if (!ok) hint = "Обход не помог — возможно, блокировка по IP, попробуйте другую стратегию";
    box.className = `site-result ${ok ? "ok" : "bad"}`;
    box.innerHTML = `<div class="msg"><b>${ok ? "✓" : "✗"} ${esc(r.host)}</b> ${ok ? `открывается · ${sec} с` : esc(r.result.error || "не открывается")}
      <br /><span>${[where, hint].filter(Boolean).join(" · ")}</span></div>
      ${r.listed ? "" : `<button class="btn small${ok ? "" : " primary"}" data-add="${esc(r.host)}">В «Обходить»</button>`}`;
  } catch (e) {
    box.className = "site-result bad";
    box.innerHTML = `<div class="msg"><b>✗</b> ${esc(e)}</div>`;
  }
  $("#btn-site").disabled = false;
}

/* ───────────── журнал ───────────── */

function logLine(line) {
  const m = /^(\d\d:\d\d:\d\d) ?(.*)$/s.exec(line);
  let [time, text] = m ? [m[1], m[2]] : ["", line];
  // строки движка бэкенд помечает префиксом "» " — их можно скрыть фильтром
  const engine = text.startsWith("» ");
  if (engine) text = text.slice(2);
  const row = document.createElement("div");
  let cls = "";
  if (text.includes("★")) cls = "star";
  else if (text.includes("✓")) cls = "ok";
  else if (/✗|Ошибка|ошибка|не удалось/.test(text)) cls = "bad";
  else if (/▶|■|Без обхода|Сторож|ИИ/.test(text)) cls = "hl";
  if (engine) cls += " engine";
  row.className = "ln " + cls;
  row.innerHTML = `<time>${time}</time><i></i><span></span>`;
  row.lastChild.textContent = text;
  const log = $("#log");
  const atBottom = log.scrollTop + log.clientHeight >= log.scrollHeight - 30;
  log.appendChild(row);
  while (log.childNodes.length > 800) log.removeChild(log.firstChild);
  if (atBottom) log.scrollTop = log.scrollHeight;
}

/* ───────────── настройки ───────────── */

function renderSettings() {
  $$("[data-key]").forEach((el) => {
    const v = settings[el.dataset.key];
    if (el.type === "checkbox") el.checked = !!v;
    else if (el.type === "time") el.value = v || "03:00";
    else $$("button", el).forEach((b) => b.classList.toggle("on", b.dataset.v === v));
  });
  $("#sched-time-row").closest(".card").classList.toggle("sched-off", !settings.schedule);
}

async function saveSetting(key, value) {
  settings = { ...settings, [key]: value };
  renderSettings();
  if (key === "theme") applyTheme(value);
  if (key.startsWith("schedule")) renderStatus();
  await call("set_settings", { settings });
}

/* ───────────── обновление Prosvet ───────────── */

function renderAppUpdate() {
  const u = status.app_update;
  $("#app-cur").textContent = info?.version || "";
  let text = appChecked ? "Установлена последняя версия ✓" : "Новые версии — из релизов GitHub, с проверкой SHA-256";
  if (u) {
    const mb = u.size ? ` · ${(u.size / 1048576).toFixed(1)} МБ` : "";
    text = u.progress != null ? `Скачиваю ${u.version}… ${u.progress}%` : `Доступна версия ${u.version}${mb}`;
  }
  $("#app-update-text").textContent = text;
  $("#btn-app-install").hidden = !u;
  $("#btn-app-install").disabled = u?.progress != null;
}

async function checkApp() {
  $("#btn-app-check").disabled = true;
  try {
    const u = await call("check_app_update");
    appChecked = !u;
    toast(u ? `Доступна версия ${u.version}` : "У вас последняя версия");
    renderAppUpdate();
  } catch {}
  $("#btn-app-check").disabled = false;
}

async function installApp() {
  toast("Скачиваю обновление — Prosvet перезапустится сам", 4000);
  try {
    await call("install_app_update");
  } catch {}
}

function openNotes() {
  const v = status.app_update?.version;
  const repo = info?.repo;
  if (repo) api.open(`https://github.com/${repo}/releases${v ? `/tag/v${v}` : ""}`);
}

async function loadInfo() {
  info = await api.invoke("get_info");
  const e = info.engines || {};
  $("#engine-versions").textContent = `zapret2 ${e.zapret2 || "—"} · Flowseal ${e.flowseal || "—"}`;
  $("#defender-text").textContent = info.defender_ok ? "Исключение добавлено ✓" : "Исключения нет — антивирус может удалить движок";
  $("#btn-defender").hidden = info.defender_ok;
  $("#data-dir").textContent = info.data_dir;
  $("#app-version").textContent = info.version;
  // Linux: нет Defender и пока нет разблокировки ИИ (она на NRPT Windows)
  if (info.os && info.os !== "windows") {
    document.body.dataset.os = info.os;
    $("#btn-defender").closest(".field").hidden = true;
    $("[data-key=ai_unlock]").closest(".card").hidden = true;
    const auto = $("[data-key=autostart]").closest(".toggle");
    auto.querySelector("b").textContent = "Запускать вместе с системой";
    auto.querySelector("small").textContent = "Тихо, в трее. Права администратора спросит при включении обхода";
  }
  renderBanners();
  renderAppUpdate();
}

async function addDefender() {
  await call("add_defender_exclusion");
  toast("Исключение добавлено");
  await loadInfo();
}

/* ════════════════════════════════════════════════════════════════
   Приветствие — начинается в темноте, свет «включается» вместе с ним
   ════════════════════════════════════════════════════════════════ */

let onbStep = -1;
let onbSelecting = false;
let onbSeenBusy = false;

function onbShow() {
  document.body.classList.add("onboarding");
  $("#onb").hidden = false;
  onbGo(0);
}

function onbGo(n) {
  onbStep = n;
  $$(".onb-step").forEach((s) => {
    const i = +s.dataset.step;
    s.classList.toggle("active", i === n);
    s.classList.toggle("past", i < n);
  });
  $$("#onb-dots i").forEach((d, i) => d.classList.toggle("on", i === n));
  if (n === 2) onbChecks();
  syncScene();
}

async function onbChecks() {
  const def = $("#onb-defender");
  const con = $("#onb-conflicts");
  setCheck(def, "wait", "Проверяю…");
  setCheck(con, "wait", "Проверяю…");
  try {
    if (!info) await loadInfo();
  } catch {}
  if (info && info.defender_ok) setCheck(def, "ok", "Исключение уже есть — движок в безопасности");
  else setCheck(def, "warn", "Антивирус может по ошибке удалить движок", "Добавить исключение");
  const c = status.conflicts || [];
  if (c.length) setCheck(con, "warn", `Найден: ${c.map((x) => x.title).join(", ")}`, "Остановить");
  else setCheck(con, "ok", "Не найдено — ничего не помешает");
}

function setCheck(el, state, text, action) {
  $(".check-icon", el).className = "check-icon " + state;
  $(".check-text span", el).textContent = text;
  const b = $("button", el);
  if (b) {
    b.hidden = !action;
    if (action) b.textContent = action;
  }
}

async function onbStart() {
  const autostart = $("#onb-autostart").checked;
  if (autostart !== settings.autostart) {
    try {
      await saveSetting("autostart", autostart);
    } catch {}
  }
  onbGo(3);
  $("#onb-kicker").textContent = "Подбор стратегии";
  $("#onb-title").textContent = "Ищу просвет…";
  $("#onb-sub").textContent = "Смотрю, что заблокировано у вашего провайдера";
  $("#onb-done").hidden = true;
  $("#onb-retry").hidden = true;
  $("#onb-skip").hidden = false;
  $("#onb-skip").textContent = "Пропустить";
  $("#onb-progress").hidden = false;
  onbSelecting = true;
  onbSeenBusy = false;
  api.invoke("auto_select").catch(() => {});
}

function onbStatus() {
  if (onbStep !== 3) return;
  const p = status.progress;
  if (onbSelecting && status.mode === "testing" && status.message) $("#onb-sub").textContent = status.message;
  if (p) {
    $("#onb-fill").style.width = `${Math.round((p.done / Math.max(p.total, 1)) * 100)}%`;
    $("#onb-ptext").textContent = p.done ? `${p.done + 1} из ${p.total}` : "проверяю без обхода";
  }
  $("#onb-chips").innerHTML = Object.entries(SERVICES)
    .map(([id, m]) => `<span class="chip ${svcState((status.services || []).find((x) => x.id === id))[0]}">${m.name}</span>`)
    .join("");

  if (!onbSelecting) return;
  if (busy(status.mode)) onbSeenBusy = true;
  if (!onbSeenBusy) return;
  if (status.mode === "on") {
    onbSelecting = false;
    $("#onb-kicker").textContent = "Готово";
    $("#onb-title").textContent = "Свет есть";
    $("#onb-sub").textContent = status.strategy
      ? `Стратегия «${status.strategy.name}». Prosvet уходит в трей и дальше следит сам.`
      : "Prosvet уходит в трей и дальше следит сам.";
    $("#onb-progress").hidden = true;
    $("#onb-done").hidden = false;
    $("#onb-skip").hidden = true;
  } else if (status.mode === "error" || status.mode === "off") {
    onbSelecting = false;
    $("#onb-kicker").textContent = "Не вышло";
    $("#onb-title").textContent = "Просвета нет";
    $("#onb-sub").textContent = status.message || "Подбор остановлен.";
    $("#onb-progress").hidden = true;
    $("#onb-retry").hidden = false;
    $("#onb-skip").textContent = "Продолжить без обхода";
  }
}

async function onbFinish() {
  const el = $("#onb");
  el.classList.add("leaving");
  onbStep = -1;
  document.body.classList.remove("onboarding");
  syncScene();
  setTimeout(() => {
    el.hidden = true;
    el.classList.remove("leaving");
    moveInk();
  }, 560);
  try {
    await saveSetting("onboarded", true);
  } catch {}
}

/* ════════════════════════════════════════════════════════════════
   События
   ════════════════════════════════════════════════════════════════ */

function bind() {
  $("#win-min").onclick = () => api.minimize();
  $("#win-close").onclick = () => api.invoke("close_main");
  $$(".tab").forEach((t) => (t.onclick = () => go(t.dataset.page)));
  // Esc — в трей, Ctrl+Enter — вкл/выкл, F5 — проверить доступ, Ctrl+1…5 — вкладки
  addEventListener("keydown", (e) => {
    const typing = /^(TEXTAREA|INPUT)$/.test(document.activeElement?.tagName || "");
    if (e.key === "Escape" && !typing) api.invoke("close_main");
    else if (e.key === "Enter" && e.ctrlKey && !typing) (e.preventDefault(), $("#power").click());
    else if (e.key === "F5") {
      e.preventDefault(); // в WebView F5 перезагрузил бы интерфейс
      if (!$("#btn-check").disabled) $("#btn-check").click();
    } else if (e.ctrlKey && !e.altKey && /^[1-5]$/.test(e.key)) {
      const tab = $$(".tab")[+e.key - 1];
      if (tab) (e.preventDefault(), go(tab.dataset.page));
    }
  });
  addEventListener("resize", moveInk);
  document.fonts?.ready.then(moveInk);

  $("#power").onclick = () => {
    const m = status.mode;
    if (m === "updating") return;
    if (m === "on" || busy(m)) api.invoke("disable");
    else call("enable").catch(() => {});
  };
  $("#btn-check").onclick = async () => {
    $("#btn-check").disabled = true;
    try {
      const r = await call("check_now");
      toast(`Доступно ${r.passed} из ${r.total}`);
    } catch {}
    $("#btn-check").disabled = busy(status.mode);
  };
  $("#btn-reselect").onclick = () => call("auto_select").catch(() => {});
  $("#btn-auto").onclick = () => {
    call("auto_select").catch(() => {});
    go("home");
  };
  $("#btn-cancel").onclick = () => api.invoke("cancel");

  $("#banners").onclick = (e) => {
    const act = e.target.closest("[data-act]")?.dataset.act;
    if (act === "resolve") api.invoke("resolve_conflicts").then(() => toast("Остановлено"));
    if (act === "defender") addDefender();
    if (act === "ai-off") saveSetting("ai_unlock", false);
    if (act === "app-install") installApp();
    if (act === "app-notes") openNotes();
    if (act === "app-later") (appLater = status.app_update?.version), renderBanners();
    if (act === "updated-ok") api.invoke("dismiss_updated");
  };

  $("#strategy-filter").onclick = (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    stratFilter = b.dataset.f;
    $$("#strategy-filter button").forEach((x) => x.classList.toggle("on", x === b));
    loadStrategies();
  };
  $("#strategy-list").onclick = (e) => {
    const id = e.target.closest("[data-run]")?.dataset.run;
    if (id) call("select_strategy", { id }).then(() => toast("Стратегия включена")).catch(() => {});
  };

  $("#list-tabs").onclick = (e) => {
    const b = e.target.closest("button");
    if (b) {
      currentList = b.dataset.list;
      loadList();
    }
  };
  $("#btn-save-list").onclick = () => saveList().catch(() => {});
  $("#list-editor").oninput = () => {
    $("#list-saved").textContent = "";
    listChanged();
  };
  $("#list-editor").onkeydown = (e) => {
    if (e.ctrlKey && e.code === "KeyS") {
      e.preventDefault();
      saveList().catch(() => {});
    }
  };

  $("#log-filter").onclick = (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    $$("#log-filter button").forEach((x) => x.classList.toggle("on", x === b));
    $("#log").dataset.f = b.dataset.f;
    $("#log").scrollTop = $("#log").scrollHeight;
  };
  $("#btn-diag").onclick = async () => {
    $("#btn-diag").disabled = true;
    try {
      await call("export_diagnostics");
      toast("Отчёт сохранён — приложите его к сообщению об ошибке", 4200);
    } catch {}
    $("#btn-diag").disabled = false;
  };
  $("#btn-pause").onclick = () => ($("#pause-opts").hidden = !$("#pause-opts").hidden);
  $("#pause-opts").onclick = (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    $("#pause-opts").hidden = true;
    call("pause_bypass", { minutes: +b.dataset.min }).catch(() => {});
  };
  $("#site-form").onsubmit = (e) => (e.preventDefault(), checkSite());
  $("#site-result").onclick = async (e) => {
    const host = e.target.closest("[data-add]")?.dataset.add;
    if (!host) return;
    try {
      await call("add_site", { host });
      toast(`${host} добавлен в «Обходить»`);
      if (currentList === "list-general-user.txt") await loadList();
      checkSite();
    } catch {}
  };
  $("#btn-copy-log").onclick = async () => {
    const text = $$("#log .ln").map((r) => `${r.firstChild.textContent} ${r.lastChild.textContent}`).join("\n");
    await navigator.clipboard.writeText(text);
    toast("Журнал скопирован");
  };

  $$(".toggle input[data-key]").forEach((el) => (el.onchange = () => saveSetting(el.dataset.key, el.checked).catch(renderSettings)));
  $$("input[type=time][data-key]").forEach((el) => {
    el.onchange = () => el.value && saveSetting(el.dataset.key, el.value).catch(renderSettings);
  });
  $$(".seg[data-key]").forEach((seg) => {
    seg.onclick = (e) => {
      const b = e.target.closest("button");
      if (b) saveSetting(seg.dataset.key, b.dataset.v).catch(renderSettings);
    };
  });

  $("#btn-check-upd").onclick = async () => {
    const [cur, latest] = await call("check_updates");
    const same = cur.flowseal === latest.flowseal && cur.zapret2 === latest.zapret2;
    toast(same ? "У вас последние версии" : `Доступно: zapret2 ${latest.zapret2}, Flowseal ${latest.flowseal}`, 4000);
  };
  $("#btn-update").onclick = async () => {
    $("#btn-update").disabled = true;
    try {
      toast(await call("update_engines"), 4000);
      await loadInfo();
    } catch {}
    $("#btn-update").disabled = false;
  };
  $("#btn-defender").onclick = addDefender;
  $("#btn-app-check").onclick = checkApp;
  $("#btn-app-install").onclick = installApp;
  $("#app-notes-link").onclick = (e) => (e.preventDefault(), openNotes());
  $("#btn-open-dir").onclick = () => api.invoke("open_data_dir");
  $$("[data-url]").forEach((a) => (a.onclick = (e) => (e.preventDefault(), api.open(a.dataset.url))));

  $$("[data-next]").forEach((b) => (b.onclick = () => onbGo(onbStep + 1)));
  $$("[data-prev]").forEach((b) => (b.onclick = () => onbGo(onbStep - 1)));
  $("#onb-go").onclick = onbStart;
  $("#onb-retry").onclick = onbStart;
  $("#onb-done").onclick = onbFinish;
  $("#onb-skip").onclick = onbFinish;
  $("#onb-defender button").onclick = async () => {
    try {
      await addDefender();
    } catch {}
    onbChecks();
  };
  $("#onb-conflicts button").onclick = async () => {
    await api.invoke("resolve_conflicts");
    status = await api.invoke("get_status");
    onbChecks();
  };
}

async function init() {
  bind();
  settings = await api.invoke("get_settings");
  applyTheme(settings.theme);
  renderSettings();
  status = await api.invoke("get_status");
  if (!settings.onboarded) onbShow();
  renderStatus();
  moveInk();
  (await api.invoke("get_logs")).forEach(logLine);

  api.listen("status", (s) => {
    status = s;
    renderStatus();
  });
  api.listen("log", logLine);
  loadInfo().catch(() => {});
  if (!T) previewFromHash();
}

init();

/* ════════════════════════════════════════════════════════════════
   Мок для просмотра дизайна в браузере (без Tauri)
   #dark | #light, &page=strategies, &bottom, &onb=0..3, &run, &upd, &site=rutracker.org
   ════════════════════════════════════════════════════════════════ */
function previewFromHash() {
  const h = new URLSearchParams(location.hash.slice(1));
  // ?shot — съёмка скриншотов: без CSS-переходов, иначе кадр ловит их середину
  if (new URLSearchParams(location.search).has("shot")) {
    const st = document.createElement("style");
    st.textContent = "*,*::before,*::after{transition:none!important;animation-duration:0s!important;animation-delay:0s!important}";
    document.head.appendChild(st);
  }
  if (h.has("dark")) applyTheme("dark");
  if (h.has("light")) applyTheme("light");
  if (h.get("page")) go(h.get("page"));
  if (h.has("bottom")) setTimeout(() => ($(".page.active").scrollTop = 1e5), 400);
  if (h.has("onb")) {
    onbShow();
    const n = +h.get("onb");
    if (n === 3) onbStart();
    else onbGo(n);
  }
  if (h.has("run")) api.invoke("enable");
  if (h.has("upd")) api.invoke("check_app_update");
  if (h.get("site")) {
    go("lists");
    $("#site-input").value = h.get("site");
    checkSite();
  }
}

function mockApi() {
  const handlers = {};
  const emit = (ev, p) => (handlers[ev] || []).forEach((f) => f(p));
  let st = { mode: "off", services: [], conflicts: [], strategy: null, progress: null, message: null, checked_at: null };
  let set = { enable_on_launch: true, autostart: false, watchdog: true, watchdog_interval: 180, fast_select: true, close_to_tray: true, game_filter: "off", ipset: "none", theme: "system", onboarded: true, ai_unlock: true, ai_provider: "auto", schedule: true, schedule_time: "03:00", schedule_update: false, app_update_check: true, notifications: true };
  const strategies = [
    ["z2-seqovl-google", "Seqovl · Google", "z2", "multisplit с перекрытием seq фейковым ClientHello google.com"],
    ["z2-fake-disorder-md5", "Fake + disorder · MD5", "z2", "фейк с подменой SNI и MD5-подписью, затем сегменты в обратном порядке"],
    ["fs:general", "Flowseal · General", "z1", "winws: fake, multisplit"],
    ["fs:general (ALT)", "Flowseal · ALT", "z1", "winws: fake, fakedsplit"],
    ["z2-multidisorder", "Multidisorder · SNI", "z2", "сегменты в обратном порядке с разрезом на имени сайта"],
    ["fs:general (FAKE TLS AUTO)", "Flowseal · FAKE TLS AUTO", "z1", "winws: fake с автоматическим TLS, multisplit"],
  ];
  const MOCK_SCORES = [[11, 2300], [8, 2700], [5, 3100], null, [11, 2900], [3, 4200]];
  const svc = (ok) => Object.keys(SERVICES).map((id, i) => ({ id, name: SERVICES[id].name, ok: ok ? 2 : i === 2 ? 2 : 0, total: 2, ms: 120 + i * 40 }));
  const push = (patch) => {
    st = { ...st, ...patch };
    emit("status", st);
  };
  const select = () => {
    let i = 0;
    push({ mode: "testing", message: "Проверяю, что заблокировано…", progress: { done: 0, total: 29, name: "без обхода" }, services: svc(false) });
    const t = setInterval(() => {
      i++;
      if (i > 4) {
        clearInterval(t);
        push({ mode: "on", message: null, progress: null, strategy: { id: "z2-seqovl-google", name: "Seqovl · Google" }, services: svc(true), checked_at: Date.now() / 1000, ai: { state: "ok" } });
        emit("log", "12:00:05 ★ выбрана «Seqovl · Google» (11/11)");
        return;
      }
      push({ message: `Пробую «${strategies[i % 4][1]}»`, progress: { done: i, total: 29, name: strategies[i % 4][1], best: i > 2 ? "Seqovl · Google · 9/11" : null } });
    }, 1000);
  };
  const cmds = {
    get_status: () => st,
    get_settings: () => set,
    set_settings: ({ settings }) => void (set = settings),
    get_logs: () => [
      "12:00:00 ▶ Prosvet 0.2.0, движки zapret2 v1.0.5.2 · Flowseal 1.10.3",
      "12:00:01 Без обхода доступно 2/11",
      "12:00:02 » capture is started.",
      "12:00:04 ✗ Fake + disorder · MD5 — 8/11, YouTube: таймаут после 16 КБ",
      "12:00:06 ✓ Seqovl · Google — 11/11 за 2.3 с",
      "12:00:06 ★ выбрана «Seqovl · Google» (11/11)",
      "12:00:07 ИИ: умный DNS Comss, Gemini и ChatGPT доступны",
      "12:03:07 Сторож: всё работает 11/11",
    ],
    get_strategies: () => strategies.map(([id, name, engine, hint], i) => ({ id, name, engine, hint, active: st.strategy?.id === id, score: MOCK_SCORES[i] && { passed: MOCK_SCORES[i][0], total: 11, latency_ms: MOCK_SCORES[i][1] } })),
    check_app_update: () => {
      push({ app_update: { version: "0.4.0", notes: "", size: 9_400_000, progress: null } });
      return st.app_update;
    },
    install_app_update: () =>
      new Promise((ok) => {
        let p = 0;
        const t = setInterval(() => {
          p = Math.min(100, p + 9);
          push({ app_update: { ...st.app_update, progress: p } });
          if (p === 100) (clearInterval(t), ok());
        }, 250);
      }),
    dismiss_updated: () => push({ updated_to: null }),
    pause_bypass: ({ minutes }) => push({ mode: "off", progress: null, paused_until: Date.now() / 1000 + minutes * 60 }),
    check_site: async ({ url }) => {
      await new Promise((r) => setTimeout(r, 700));
      const host = url.replace(/^\w+:\/\//, "").replace(/^www\./, "").split("/")[0].toLowerCase();
      const ok = host.length % 2 === 0;
      return { host, listed: host.includes("youtube") ? "список Flowseal" : null, result: { ok, ms: 830, error: ok ? null : "таймаут после 16 КБ" } };
    },
    add_site: () => null,
    export_diagnostics: () => "C:\\Users\\you\\AppData\\Local\\Prosvet\\diagnostics\\prosvet-diag.zip",
    get_info: () => ({ version: "0.3.1", repo: "Ivantech123/Prosvet_antizapret_app", engines: { flowseal: "1.10.3", zapret2: "v1.0.5.2" }, data_dir: "C:\\Users\\you\\AppData\\Local\\Prosvet", defender_ok: true }),
    enable: () => select(),
    auto_select: () => select(),
    disable: () => push({ mode: "off", progress: null }),
    cancel: () => push({ mode: "off", progress: null }),
    check_now: () => ({ passed: 11, total: 11 }),
    get_list: () => "# Свои домены\nrutracker.org\n",
  };
  return {
    invoke: async (c, a) => (cmds[c] ? cmds[c](a || {}) : null),
    listen: (ev, fn) => (handlers[ev] = handlers[ev] || []).push(fn),
    minimize: () => {},
    open: (u) => window.open(u),
  };
}
