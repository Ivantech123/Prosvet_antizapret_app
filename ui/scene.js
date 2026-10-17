"use strict";

/* ════════════════════════════════════════════════════════════════
   Сцена «Луч»: из щели наверху бьёт объёмный луч света, в нём
   плавают пылинки. Состояние приложения меняет «погоду» сцены:
     off      — щель едва тлеет, луча почти нет
     testing  — холодный луч ищет, раскачиваясь
     on       — тёплый золотой поток, пылинки текут вниз
     error    — красноватый тусклый свет
   Объёмный луч рисуется в буфере в 1/4 разрешения и растягивается —
   это даёт мягкое размытие почти бесплатно.
   ════════════════════════════════════════════════════════════════ */

const PRESETS = {
  dark: {
    off: { i: 0.1, slit: 0.32, spread: 0.26, sweep: 0, speed: 0.22, c: [150, 162, 190] },
    testing: { i: 0.55, slit: 0.95, spread: 0.24, sweep: 0.12, speed: 0.55, c: [130, 175, 255] },
    on: { i: 1, slit: 1, spread: 0.4, sweep: 0, speed: 1, c: [255, 192, 104] },
    error: { i: 0.3, slit: 0.6, spread: 0.28, sweep: 0, speed: 0.18, c: [255, 104, 104] },
    intro: { i: 0.85, slit: 1, spread: 0.36, sweep: 0, speed: 0.6, c: [255, 200, 120] },
  },
  light: {
    off: { i: 0.16, slit: 0.4, spread: 0.26, sweep: 0, speed: 0.22, c: [120, 124, 140] },
    testing: { i: 0.6, slit: 0.95, spread: 0.24, sweep: 0.12, speed: 0.55, c: [70, 125, 235] },
    on: { i: 0.62, slit: 1, spread: 0.34, sweep: 0, speed: 1, c: [246, 170, 70] },
    error: { i: 0.4, slit: 0.6, spread: 0.28, sweep: 0, speed: 0.18, c: [225, 70, 70] },
    intro: { i: 0.55, slit: 1, spread: 0.32, sweep: 0, speed: 0.6, c: [246, 172, 80] },
  },
};

class Scene {
  constructor(canvas) {
    this.cv = canvas;
    this.ctx = canvas.getContext("2d");
    this.buf = document.createElement("canvas");
    this.bctx = this.buf.getContext("2d");
    this.mode = "off";
    this.theme = "dark";
    // текущие параметры стартуют с «темноты», чтобы при запуске свет включался
    this.cur = { i: 0, slit: 0, spread: 0.2, sweep: 0, speed: 0.2, c: [150, 162, 190] };
    this.flash = 0;
    this.t = 0;
    this.last = performance.now();
    this.particles = [];
    this.resize();
    addEventListener("resize", () => this.resize());
    const mq = matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener?.("change", () => this.syncTheme());
    this.syncTheme();
    requestAnimationFrame((t) => this.frame(t));
  }

  syncTheme() {
    const forced = document.documentElement.dataset.theme;
    this.theme = forced || (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  }

  set(mode) {
    if (mode === this.mode) return;
    // включение — щель вспыхивает, как лампа
    if (mode === "on" && this.mode !== "on") this.flash = 1;
    this.mode = mode;
  }

  resize() {
    const dpr = Math.min(devicePixelRatio || 1, 2);
    this.w = innerWidth;
    this.h = innerHeight;
    this.cv.width = Math.round(this.w * dpr);
    this.cv.height = Math.round(this.h * dpr);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.buf.width = Math.max(1, Math.round(this.w / 4));
    this.buf.height = Math.max(1, Math.round(this.h / 4));
    const n = Math.round(Math.min(180, (this.w * this.h) / 5200));
    this.particles = Array.from({ length: n }, () => this.spawn(true));
  }

  spawn(anywhere) {
    return {
      x: Math.random() * this.w,
      y: anywhere ? Math.random() * this.h : this.slitY() + Math.random() * 40,
      z: Math.random(), // глубина: размер и скорость
      ph: Math.random() * Math.PI * 2,
    };
  }

  slitY() {
    return Math.max(118, this.h * 0.19);
  }

  /** Насколько точка внутри луча (0..1). */
  inBeam(x, y, P) {
    const top = this.slitY();
    if (y < top) return 0;
    const k = (y - top) / (this.h - top);
    const cx = this.w / 2 + Math.sin(this.angle) * (y - top);
    const hw = 6 + k * P.spread * this.w;
    const d = Math.abs(x - cx) / hw;
    return Math.max(0, 1 - d) ** 1.4 * (1 - k * 0.55);
  }

  frame(now) {
    const dt = Math.min(0.05, (now - this.last) / 1000);
    this.last = now;
    this.t += dt;
    if (!document.hidden) this.draw(dt);
    requestAnimationFrame((t) => this.frame(t));
  }

  draw(dt) {
    const target = PRESETS[this.theme][this.mode] || PRESETS.dark.off;
    const P = this.cur;
    const k = Math.min(1, dt * 1.8);
    for (const key of ["i", "slit", "spread", "sweep", "speed"]) P[key] += (target[key] - P[key]) * k;
    P.c = P.c.map((v, j) => v + (target.c[j] - v) * k);
    this.flash = Math.max(0, this.flash - dt * 1.4);

    // мерцание при включении + лёгкое «дыхание» света
    const flicker = this.flash > 0.55 ? (Math.random() < 0.5 ? 0.35 : 1.25) : 1;
    const breathe = 1 + Math.sin(this.t * 0.9) * 0.04;
    const I = P.i * flicker * breathe;
    this.angle = Math.sin(this.t * 1.3) * P.sweep * 0.5;

    const { ctx, w, h } = this;
    const dark = this.theme === "dark";
    const [r, g, b] = P.c.map(Math.round);
    const rgba = (a) => `rgba(${r},${g},${b},${Math.max(0, Math.min(1, a))})`;
    const top = this.slitY();
    const cx = w / 2;

    ctx.clearRect(0, 0, w, h);

    // ── объёмный луч в буфере 1/4 ──
    const B = this.bctx;
    const bw = this.buf.width;
    const bh = this.buf.height;
    const s = bw / w;
    B.clearRect(0, 0, bw, bh);
    B.save();
    B.translate(cx * s, top * s);
    B.rotate(-this.angle);
    const layers = [
      [1.5, 0.16],
      [1.0, 0.3],
      [0.45, 0.42],
      [0.16, 0.5],
    ];
    for (const [mul, a] of layers) {
      const half = P.spread * w * mul * s;
      const len = (h - top) * s * 1.05;
      const grad = B.createLinearGradient(0, 0, 0, len);
      grad.addColorStop(0, rgba(a * I * (dark ? 1 : 0.7)));
      grad.addColorStop(0.55, rgba(a * I * (dark ? 0.45 : 0.22)));
      grad.addColorStop(1, rgba(0));
      B.fillStyle = grad;
      B.beginPath();
      B.moveTo(-1.5 * s * 4, 0);
      B.lineTo(1.5 * s * 4, 0);
      B.lineTo(half, len);
      B.lineTo(-half, len);
      B.closePath();
      B.fill();
    }
    B.restore();

    ctx.save();
    ctx.globalCompositeOperation = dark ? "lighter" : "source-over";
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(this.buf, 0, 0, w, h);

    // свечение вокруг щели
    const halo = ctx.createRadialGradient(cx, top + 30, 0, cx, top + 30, 260);
    halo.addColorStop(0, rgba(0.42 * I));
    halo.addColorStop(1, rgba(0));
    ctx.fillStyle = halo;
    ctx.fillRect(cx - 260, top - 230, 520, 520);

    // пятно света на «полу»
    const fx = cx + Math.sin(this.angle) * (h - top);
    const floor = ctx.createRadialGradient(fx, h + 40, 0, fx, h + 40, P.spread * w * 1.3);
    floor.addColorStop(0, rgba(0.22 * I));
    floor.addColorStop(1, rgba(0));
    ctx.fillStyle = floor;
    ctx.fillRect(0, h * 0.5, w, h * 0.5 + 40);

    // ── пылинки ──
    for (const p of this.particles) {
      p.y += (8 + p.z * 26) * P.speed * dt;
      p.x += Math.sin(this.t * 0.6 + p.ph) * 4 * dt;
      if (p.y > h + 4) Object.assign(p, this.spawn(false));
      const inside = this.inBeam(p.x, p.y, P);
      const a = (dark ? 0.05 : 0.08) + inside * I * (0.55 + p.z * 0.45);
      if (a < 0.03) continue;
      const rad = 0.5 + p.z * 1.5;
      ctx.fillStyle = inside > 0.05 ? rgba(a) : dark ? `rgba(220,225,240,${a})` : `rgba(90,90,110,${a})`;
      ctx.beginPath();
      ctx.arc(p.x, p.y, rad, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();

    // ── сама щель ──
    const slitA = Math.min(1, P.slit * flicker);
    const sh = 66;
    ctx.save();
    ctx.translate(cx, top);
    ctx.shadowColor = rgba(0.9 * slitA);
    ctx.shadowBlur = 28 * slitA;
    const core = ctx.createLinearGradient(0, -sh, 0, 6);
    const white = dark ? "255,250,240" : "255,255,255";
    core.addColorStop(0, `rgba(${white},0)`);
    core.addColorStop(0.35, `rgba(${white},${0.9 * slitA})`);
    core.addColorStop(1, `rgba(${white},${slitA})`);
    ctx.fillStyle = slitA > 0.05 ? core : "transparent";
    roundRect(ctx, -2.5, -sh, 5, sh + 4, 2.5);
    ctx.fill();
    if (!dark || slitA < 0.4) {
      // в выключенном виде щель — тёмная прорезь
      ctx.shadowBlur = 0;
      ctx.fillStyle = dark ? `rgba(120,128,150,${0.35 * (1 - slitA)})` : `rgba(40,40,50,${0.5 * (1 - slitA)})`;
      roundRect(ctx, -2.5, -sh, 5, sh + 4, 2.5);
      ctx.fill();
    }
    ctx.restore();
  }
}

function roundRect(ctx, x, y, w, h, r) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

window.Scene = Scene;
