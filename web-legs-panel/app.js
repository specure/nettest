// Nettest web client, "Orbit" design: drives the Rust/WASM measurement
// (`run_measurement`) and draws the whole test as one ring of samples.
// Plain ES module, no framework, no build step.
//
// Main-thread budget: the measurement runs on this thread too. The ring is
// the only per-frame drawing; it runs only while something moves, is capped
// at 10 fps during transfers (lower if a frame gets expensive) and is off
// entirely with ?fx=0 or "reduce motion".

import init, { run_measurement } from './nettest.js';

const $ = (id) => document.getElementById(id);
const body = document.body;

// ---------------------------------------------------------------------------
// Settings: defaults < localStorage < URL query (?threads=8&autostart=0 …)
// ---------------------------------------------------------------------------

const DEFAULTS = Object.freeze({
  threads: 3,
  downloadS: 7,
  uploadS: 7,
  autostart: true,
  fx: true,
  internet: true,
  pretest: true,
  ping: true,
  download: true,
  upload: true,
  qos: true,
  signedResult: false,
  chunkSize: 0,
  maxChunkSize: '',
  boundedDownload: true,
  downloadWindowMiB: 96,
  legacyUpload: false,
  putTimeResultIntervalMs: 0,
  qosPackets: 10,
  qosDelayMs: 200,
  server: '',
  save: false,
  controlServer: 'https://api.nettest.org',
});

const STORE_SETTINGS = 'nettest.settings.v1';
const STORE_HISTORY = 'nettest.history.v1';
const HISTORY_MAX = 100;

const storage = (() => {
  try {
    const k = '__nettest_probe__';
    localStorage.setItem(k, '1');
    localStorage.removeItem(k);
    return localStorage;
  } catch {
    return null; // private mode / disabled storage: work without persistence
  }
})();

function readJson(key, fallback) {
  try {
    const raw = storage?.getItem(key);
    return raw ? JSON.parse(raw) : fallback;
  } catch {
    return fallback;
  }
}

function writeJson(key, value) {
  try {
    storage?.setItem(key, JSON.stringify(value));
  } catch { /* quota or disabled: ignore */ }
}

/** Coerce `value` to the type of the default for `key`. */
function coerce(key, value) {
  const def = DEFAULTS[key];
  if (typeof def === 'boolean') {
    if (typeof value === 'boolean') return value;
    return /^(1|true|yes|on)$/i.test(String(value));
  }
  if (typeof def === 'number') {
    const n = Number(value);
    return Number.isFinite(n) ? n : def;
  }
  if (key === 'maxChunkSize') {
    const n = Number(value);
    return value === '' || value == null || !Number.isFinite(n) ? '' : n;
  }
  return value == null ? def : String(value);
}

function loadSettings() {
  const stored = readJson(STORE_SETTINGS, {});
  const s = { ...DEFAULTS };
  for (const key of Object.keys(DEFAULTS)) {
    if (key in stored) s[key] = coerce(key, stored[key]);
  }
  for (const [key, value] of new URLSearchParams(location.search)) {
    if (key in DEFAULTS) s[key] = coerce(key, value);
  }
  return s;
}

let settings = loadSettings();
const saveSettings = () => writeJson(STORE_SETTINGS, settings);

const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)');
const fx = () => settings.fx && !reduceMotion.matches;
const noHover = matchMedia('(hover: none)').matches;

function serverUrl() {
  if (settings.server.trim()) return settings.server.trim();
  const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
  return `${scheme}://${location.host}/`;
}

/** Packet loss needs WebTransport, which browsers only expose on secure pages. */
const lossMeasurable = () => settings.qos && window.isSecureContext && 'WebTransport' in window;

function measurementOptions() {
  const o = {
    threads: Math.max(1, Math.round(settings.threads)),
    downloadMs: Math.round(settings.downloadS * 1000),
    uploadMs: Math.round(settings.uploadS * 1000),
    pretest: settings.pretest,
    ping: settings.ping,
    download: settings.download,
    upload: settings.upload,
    // Jitter still comes from the ping phase without it; skipping a QoS
    // phase that cannot run here also skips its settle pause.
    qos: lossMeasurable(),
    signedResult: settings.signedResult,
    chunkSize: settings.chunkSize,
    boundedDownload: settings.boundedDownload,
    downloadWindowMiB: settings.downloadWindowMiB,
    legacyUpload: settings.legacyUpload,
    putTimeResultIntervalMs: settings.putTimeResultIntervalMs,
    qosPackets: settings.qosPackets,
    qosDelayMs: settings.qosDelayMs,
    save: settings.save,
  };
  if (settings.maxChunkSize !== '') o.maxChunkSize = settings.maxChunkSize;
  if (settings.save && settings.controlServer) o.controlServer = settings.controlServer;
  return o;
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

const ok = (v) => typeof v === 'number' && Number.isFinite(v);

function fmtNum(v) {
  if (!ok(v)) return '–';
  if (v < 10) return v.toFixed(2);
  if (v < 100) return v.toFixed(1);
  return Math.round(v).toString();
}
const fmtLoss = (v) => (ok(v) ? v.toFixed(v < 10 ? 1 : 0) : '–');

function fmtBytes(b) {
  if (!ok(b)) return '–';
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (b >= 1000 && i < u.length - 1) { b /= 1000; i++; }
  return `${b.toFixed(i ? 1 : 0)} ${u[i]}`;
}

function fmtTime(ts) {
  const d = new Date(ts);
  const now = new Date();
  const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (d.toDateString() === now.toDateString()) return `Today, ${time}`;
  const date = d.toLocaleDateString([], {
    day: 'numeric', month: 'short', year: d.getFullYear() === now.getFullYear() ? undefined : 'numeric',
  });
  return `${date}, ${time}`;
}

/** The metrics, in display order. `big` ones take the first row on phones. */
const METRICS = [
  { k: 'download', label: 'Download', unit: 'Mbit/s', fmt: fmtNum, big: true, higher: true },
  { k: 'upload', label: 'Upload', unit: 'Mbit/s', fmt: fmtNum, big: true, higher: true },
  { k: 'ping', label: 'Ping', unit: 'ms', fmt: fmtNum },
  { k: 'jitter', label: 'Jitter', unit: 'ms', fmt: fmtNum },
  { k: 'loss', label: 'Packet loss', unit: '%', fmt: fmtLoss },
];

// ---------------------------------------------------------------------------
// Ring renderer: a pure function, shared by the live ring, the history
// thumbnails and the exported image.
//
// One clockwise sweep from 12 o'clock: download fills the right half
// (12 → 6), upload continues up the left half (6 → 12). Each half has
// BARS radial bars, one per time slot, whose length is the throughput in
// that slot relative to the half's own peak.
// ---------------------------------------------------------------------------

const BARS = 72;
const TAU = Math.PI * 2;
const START = -Math.PI / 2;

function hexAlpha(color, a) {
  const m = /^#([0-9a-f]{6})$/i.exec(color);
  if (!m) return color;
  const n = parseInt(m[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}

function readColors() {
  const cs = getComputedStyle(document.documentElement);
  const v = (n) => cs.getPropertyValue(n).trim();
  return {
    d1: v('--down'), d2: v('--down-2'), u1: v('--up'), u2: v('--up-2'),
    ink: v('--ink'), muted: v('--muted'), faint: v('--faint'), line: v('--line-2'),
    bg: v('--bg'), bg2: v('--bg-2'),
  };
}

/** Angle of position `t` (0..1) in a half; `half` 0 = download, 1 = upload. */
const angleAt = (half, t) => START + Math.PI * (half + t);

/**
 * Paint the ring centred at (cx, cy) with outer radius R.
 * o.halves: [downLengths, upLengths], each BARS values in 0..1 or NaN (empty).
 * o.tip: { half, t } sweep position, o.radar: angle of a radar sweep,
 * o.pings: [{ a, r, alpha }] in ring units, o.flash: 0..1, o.track: dots.
 */
function paintRing(ctx, cx, cy, R, o) {
  const c = o.colors;
  const rIn = R * 0.64;
  const span = R - rIn;
  const step = Math.PI / BARS;
  const w = Math.max(1, rIn * step * 0.52);
  ctx.lineCap = 'round';

  // Hairline orbit and the two handover marks (12 and 6 o'clock).
  ctx.strokeStyle = c.line;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.arc(cx, cy, R + w * 1.6, 0, TAU);
  ctx.stroke();
  if (o.track !== false) {
    ctx.beginPath();
    for (const a of [START, START + Math.PI]) {
      ctx.moveTo(cx + Math.cos(a) * (R + w * 0.6), cy + Math.sin(a) * (R + w * 0.6));
      ctx.lineTo(cx + Math.cos(a) * (R + w * 3.2), cy + Math.sin(a) * (R + w * 3.2));
    }
    ctx.lineWidth = 1.5;
    ctx.strokeStyle = c.faint;
    ctx.stroke();
  }

  // Radar sweep (connecting, ping, quality): a soft wedge trailing an angle.
  const wedge = (angle, alpha) => {
    if (!ctx.createConicGradient) return;
    const width = 0.9;
    const g = ctx.createConicGradient(angle - width, cx, cy);
    g.addColorStop(0, hexAlpha(c.ink, 0));
    g.addColorStop(width / TAU, hexAlpha(c.ink, alpha));
    g.addColorStop(width / TAU + 0.001, hexAlpha(c.ink, 0));
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.arc(cx, cy, R + w, 0, TAU);
    ctx.arc(cx, cy, rIn - w * 2, 0, TAU, true);
    ctx.fill();
  };
  if (o.radar != null) wedge(o.radar, 0.13);

  // Empty slots: a dotted track.
  if (o.track !== false) {
    ctx.beginPath();
    for (let h = 0; h < 2; h++) {
      const arr = o.halves[h];
      for (let i = 0; i < BARS; i++) {
        if (arr && arr[i] >= 0) continue;
        const a = angleAt(h, (i + 0.5) / BARS);
        const x = cx + Math.cos(a) * rIn;
        const y = cy + Math.sin(a) * rIn;
        ctx.moveTo(x, y);
        ctx.lineTo(x + 0.01, y);
      }
    }
    ctx.strokeStyle = c.faint;
    ctx.lineWidth = Math.max(1, w * 0.55);
    ctx.stroke();
  }

  // Sample bars, one path per half: a wide faint pass for the glow, then
  // the bars themselves with a radial gradient (inner → outer colour).
  for (let h = 0; h < 2; h++) {
    const arr = o.halves[h];
    if (!arr) continue;
    const path = new Path2D();
    let any = false;
    for (let i = 0; i < BARS; i++) {
      const v = arr[i];
      if (!(v >= 0)) continue;
      any = true;
      const a = angleAt(h, (i + 0.5) / BARS);
      const len = Math.max(w * 0.4, Math.min(1, v) * span);
      const cos = Math.cos(a), sin = Math.sin(a);
      path.moveTo(cx + cos * rIn, cy + sin * rIn);
      path.lineTo(cx + cos * (rIn + len), cy + sin * (rIn + len));
    }
    if (!any) continue;
    const [a1, a2] = h === 0 ? [c.d1, c.d2] : [c.u1, c.u2];
    const g = ctx.createRadialGradient(cx, cy, rIn, cx, cy, R);
    g.addColorStop(0, a2);
    g.addColorStop(1, a1);
    ctx.strokeStyle = g;
    if (o.glow !== false) {
      ctx.globalAlpha = 0.14 + 0.35 * (o.flash || 0);
      ctx.lineWidth = w * 3.2;
      ctx.stroke(path);
    }
    ctx.globalAlpha = 1;
    ctx.lineWidth = w;
    ctx.stroke(path);
  }

  // Ping replies: dots, further out = slower round trip.
  if (o.pings?.length) {
    ctx.fillStyle = c.ink;
    for (const p of o.pings) {
      if (p.alpha <= 0) continue;
      ctx.globalAlpha = p.alpha;
      const r = rIn + p.r * span;
      ctx.beginPath();
      ctx.arc(cx + Math.cos(p.a) * r, cy + Math.sin(p.a) * r, Math.max(1.5, w * 0.62), 0, TAU);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  // Sweep tip: a trailing wedge plus a bright hand.
  if (o.tip) {
    const a = angleAt(o.tip.half, o.tip.t);
    wedge(a, 0.1);
    ctx.strokeStyle = c.ink;
    ctx.lineWidth = Math.max(1, w * 0.4);
    ctx.beginPath();
    ctx.moveTo(cx + Math.cos(a) * (rIn - w * 2), cy + Math.sin(a) * (rIn - w * 2));
    ctx.lineTo(cx + Math.cos(a) * (R + w * 1.6), cy + Math.sin(a) * (R + w * 1.6));
    ctx.stroke();
    ctx.fillStyle = c.ink;
    ctx.beginPath();
    ctx.arc(cx + Math.cos(a) * (R + w * 1.6), cy + Math.sin(a) * (R + w * 1.6), w * 0.75, 0, TAU);
    ctx.fill();
  }

  // Completion flash: the orbit lights up and fades.
  if (o.flash > 0) {
    ctx.globalAlpha = o.flash;
    ctx.strokeStyle = c.ink;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.arc(cx, cy, R + w * 1.6, 0, TAU);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
}

/** Normalise raw Mbit/s slots of one half against its own peak. */
function normalise(values, scale) {
  if (!values) return null;
  const s = scale || Math.max(0, ...values.filter(ok)) * 1.1 || 1;
  return values.map((v) => (ok(v) ? v / s : NaN));
}

// ---------------------------------------------------------------------------
// Live ring
// ---------------------------------------------------------------------------

const ring = {
  canvas: $('ring'),
  ctx: null,
  size: 0,
  colors: null,
  raw: [null, null],        // Mbit/s per slot, per half
  shown: [new Float32Array(BARS).fill(NaN), new Float32Array(BARS).fill(NaN)],
  lastIdx: [-1, -1],
  peak: [0, 0],
  scale: [1, 1],            // eased display scale per half
  tip: null,                // { half, t } while a transfer runs
  radar: false,             // radar sweep while waiting
  radarAngle: START,
  pings: [],
  pingFade: 1,
  flash: 0,
  raf: 0,
  last: 0,
  lastDraw: 0,
  frameMs: 0,               // smoothed paint cost
  minFrame: 16,

  setup() {
    this.ctx = this.canvas.getContext('2d');
    new ResizeObserver(() => this.resize()).observe(this.canvas);
    matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => {
      this.colors = null;
      this.draw();
    });
  },

  resize() {
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const r = this.canvas.getBoundingClientRect();
    this.size = r.width;
    this.canvas.width = Math.round(r.width * dpr);
    this.canvas.height = Math.round(r.height * dpr);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.draw();
  },

  clear() {
    this.raw = [null, null];
    for (const s of this.shown) s.fill(NaN);
    this.lastIdx = [-1, -1];
    this.peak = [0, 0];
    this.scale = [1, 1];
    this.tip = null;
    this.pings = [];
    this.flash = 0;
    this.kick();
  },

  /** Load a finished ring (history entry), no animation. */
  load(entry) {
    this.clear();
    const r = entry.ring || {};
    [r.d, r.u].forEach((vals, h) => {
      if (!Array.isArray(vals)) return;
      this.raw[h] = vals.map((v) => (v == null ? NaN : v));
      this.peak[h] = Math.max(0, ...this.raw[h].filter(ok));
      this.scale[h] = this.peak[h] * 1.1 || 1;
      const n = normalise(this.raw[h], this.scale[h]);
      for (let i = 0; i < BARS; i++) this.shown[h][i] = n[i];
    });
    this.draw();
  },

  /** A live throughput sample at position t (0..1) of a half. */
  sample(half, t, v) {
    if (!ok(v)) return;
    const raw = (this.raw[half] ||= new Array(BARS).fill(NaN));
    const idx = Math.min(BARS - 1, Math.max(0, Math.floor(t * BARS)));
    const from = this.lastIdx[half];
    const prev = from >= 0 ? raw[from] : v;
    // Fill skipped slots by interpolation so the ring has no gaps.
    for (let i = from + 1; i <= idx; i++) {
      const k = idx === from ? 1 : (i - from) / (idx - from);
      raw[i] = prev + (v - prev) * k;
    }
    if (idx <= from) raw[idx] = v;
    this.lastIdx[half] = Math.max(from, idx);
    this.peak[half] = Math.max(this.peak[half], v);
    this.tip = { half, t: Math.min(1, t) };
    this.kick();
  },

  /** Phase finished: extend the last value to the end of the half. */
  complete(half) {
    const raw = this.raw[half];
    if (raw) {
      const last = this.lastIdx[half];
      for (let i = last + 1; i < BARS; i++) raw[i] = raw[Math.max(0, last)];
      this.lastIdx[half] = BARS - 1;
    }
    this.tip = null;
    this.kick();
  },

  setPings(samples) {
    const vals = samples.filter(ok);
    if (!vals.length) return;
    const lo = Math.min(...vals), hi = Math.max(...vals);
    const now = performance.now();
    this.pingFade = 1;
    this.pings = vals.map((ms, i) => ({
      a: START + (TAU * (i + 0.5)) / vals.length,
      r: 0.12 + 0.8 * (hi > lo ? (ms - lo) / (hi - lo) : 0.5),
      at: now + (fx() ? (i / vals.length) * 500 : 0),
      alpha: 0,
    }));
    this.kick();
  },

  finish() {
    this.tip = null;
    this.radar = false;
    this.pings = [];
    this.flash = fx() ? 1 : 0;
    this.kick();
  },

  setRadar(on) {
    this.radar = on;
    this.kick();
  },

  kick() {
    // During transfers the measurement owns the main thread: no frame loop,
    // just one paint per progress sample (every ~150 ms).
    if (this.tip && !this.flash) {
      if (this.raf) { cancelAnimationFrame(this.raf); this.raf = 0; }
      this.pings = [];
      this.radar = false;
    }
    if (!fx() || (this.tip && !this.flash)) {
      // No animation: snap everything and paint once.
      for (let h = 0; h < 2; h++) {
        if (!this.raw[h]) continue;
        this.scale[h] = this.peak[h] * 1.1 || 1;
        const n = normalise(this.raw[h], this.scale[h]);
        for (let i = 0; i < BARS; i++) this.shown[h][i] = n[i];
      }
      for (const p of this.pings) p.alpha = 1;
      this.flash = 0;
      this.draw();
      return;
    }
    if (this.raf) return;
    this.last = performance.now();
    this.raf = requestAnimationFrame((t) => this.tick(t));
  },

  tick(now) {
    this.raf = 0;
    const dt = Math.min(100, now - this.last);
    this.last = now;
    let moving = this.radar || !!this.tip;

    for (let h = 0; h < 2; h++) {
      const raw = this.raw[h];
      if (!raw) continue;
      const target = this.peak[h] * 1.1 || 1;
      this.scale[h] += (target - this.scale[h]) * (1 - Math.exp(-dt / 350));
      if (Math.abs(target - this.scale[h]) > target * 0.002) moving = true;
      else this.scale[h] = target;
      const k = 1 - Math.exp(-dt / 120);
      const shown = this.shown[h];
      for (let i = 0; i < BARS; i++) {
        const v = raw[i];
        if (!ok(v)) { shown[i] = NaN; continue; }
        const t = v / this.scale[h];
        const s = ok(shown[i]) ? shown[i] : 0;
        const n = s + (t - s) * k;
        if (Math.abs(t - n) > 0.002) moving = true;
        shown[i] = Math.abs(t - n) > 0.002 ? n : t;
      }
    }

    if (this.radar) this.radarAngle = (this.radarAngle + dt * 0.0042) % TAU;
    for (const p of this.pings) {
      const target = now >= p.at ? this.pingFade : 0;
      p.alpha += (target - p.alpha) * (1 - Math.exp(-dt / 90));
      if (Math.abs(target - p.alpha) > 0.01) moving = true;
    }
    // Ping dots give way to the download bars.
    if (this.pings.length && this.tip) {
      this.pingFade = Math.max(0, this.pingFade - dt / 900);
      if (this.pingFade === 0 && this.pings.every((p) => p.alpha < 0.01)) this.pings = [];
    }
    if (this.flash > 0) {
      this.flash = Math.max(0, this.flash - dt / 900);
      moving = true;
    }
    core.tick(dt) && (moving = true);

    // ~10 fps while the measurement is moving data (samples only arrive a few
    // times a second anyway), slower still if painting is expensive here.
    const transfer = !!this.tip;
    this.minFrame = transfer ? (this.frameMs > 2 ? 160 : 100) : 16;
    if (now - this.lastDraw >= this.minFrame - 1 || !moving) {
      this.lastDraw = now;
      const t0 = performance.now();
      this.draw();
      this.frameMs = this.frameMs * 0.9 + (performance.now() - t0) * 0.1;
    }
    if (moving) this.raf = requestAnimationFrame((t) => this.tick(t));
  },

  draw() {
    const { ctx, size } = this;
    if (!ctx || !size) return;
    if (!this.colors) this.colors = readColors();
    ctx.clearRect(0, 0, size, size);
    const pad = size * 0.03;
    paintRing(ctx, size / 2, size / 2, size / 2 - pad, {
      colors: this.colors,
      halves: this.shown,
      tip: this.tip,
      radar: this.radar ? this.radarAngle : null,
      pings: this.pings,
      flash: this.flash,
      glow: !this.tip, // the wide glow pass doubles the stroke cost; skip it mid-transfer
    });
  },
};

// ---------------------------------------------------------------------------
// Centre readout (also the start / run-again button)
// ---------------------------------------------------------------------------

const core = {
  btn: $('core'),
  phase: $('phase-label'),
  value: $('core-value'),
  unit: $('core-unit'),
  cta: $('core-cta'),
  target: NaN,
  shown: NaN,
  fmt: fmtNum,

  set({ phase, value, unit = '', cta = '', waiting = false, button = null, fmt = fmtNum }) {
    if (phase !== undefined) this.phase.textContent = phase;
    this.phase.classList.toggle('waiting', waiting);
    this.unit.textContent = unit;
    this.cta.textContent = cta;
    this.fmt = fmt;
    if (typeof value === 'string') {
      this.target = this.shown = NaN;
      this.value.textContent = value;
    } else if (ok(value)) {
      this.number(value, true);
    } else {
      this.target = this.shown = NaN;
      this.value.textContent = '';
    }
    this.btn.disabled = !button;
    if (button) this.btn.setAttribute('aria-label', button);
  },

  /** Ease the big number towards `v` (driven by the ring's frame loop). */
  number(v, snap = false) {
    if (!ok(v)) return;
    this.target = v;
    if (snap || !fx() || ring.tip || !ok(this.shown)) {
      this.shown = v;
      this.value.textContent = this.fmt(v);
    }
    ring.kick();
  },

  tick(dt) {
    if (!ok(this.target) || !ok(this.shown) || this.shown === this.target) return false;
    this.shown += (this.target - this.shown) * (1 - Math.exp(-dt / 180));
    if (Math.abs(this.target - this.shown) < Math.max(0.005, this.target * 0.002)) this.shown = this.target;
    const text = this.fmt(this.shown);
    if (this.value.textContent !== text) this.value.textContent = text;
    return this.shown !== this.target;
  },
};

$('core').addEventListener('click', () => start());

// ---------------------------------------------------------------------------
// Results strip, steps, messages
// ---------------------------------------------------------------------------

const strip = {
  el: $('strip'),
  items: {},

  /** Lay out the given metric keys; the grid keeps any count evenly filled. */
  build(keys, keep = false) {
    const old = keep ? this.items : {};
    this.el.textContent = '';
    this.items = {};
    const list = METRICS.filter((m) => keys.includes(m.k));
    this.el.dataset.big = String(list.filter((m) => m.big).length);
    this.el.dataset.small = String(list.filter((m) => !m.big).length);
    for (const m of list) {
      const el = document.createElement('div');
      el.className = `m pending${m.big ? ' big' : ''}`;
      el.dataset.k = m.k;
      el.innerHTML = '<span class="k"><i></i></span><span class="v"><b>–</b><small></small></span><span class="d"></span>';
      el.querySelector('.k').append(m.label);
      el.querySelector('small').textContent = m.unit;
      this.el.append(el);
      this.items[m.k] = { el, m, b: el.querySelector('b'), d: el.querySelector('.d') };
      if (old[m.k]) this.set(m.k, old[m.k].v);
    }
  },

  set(k, v, live = false) {
    const it = this.items[k];
    if (!it) return;
    it.v = v;
    it.el.classList.toggle('pending', !ok(v));
    it.el.classList.toggle('live', live);
    it.b.textContent = it.m.fmt(v);
  },

  delta(k, cur, prev) {
    const it = this.items[k];
    if (!it || !ok(cur) || !ok(prev)) return;
    const diff = cur - prev;
    let text;
    if (prev === 0 || it.m.k === 'loss') {
      if (Math.abs(diff) < 0.05) text = '= last run';
      else text = `${diff > 0 ? '▲' : '▼'} ${it.m.fmt(Math.abs(diff))} ${it.m.unit}`;
    } else {
      const pct = (diff / prev) * 100;
      text = Math.abs(pct) < 1 ? '= last run' : `${pct > 0 ? '▲' : '▼'} ${Math.abs(pct) < 10 ? Math.abs(pct).toFixed(1) : Math.round(Math.abs(pct))} %`;
    }
    const same = text.startsWith('=');
    const better = it.m.higher ? diff > 0 : diff < 0;
    it.d.textContent = text;
    it.d.className = `d${same ? '' : better ? ' better' : ' worse'}`;
    it.d.title = `Last run: ${it.m.fmt(prev)} ${it.m.unit}`;
  },
};

const STEP_ORDER = ['ping', 'download', 'upload', 'qos'];

function setupSteps() {
  const on = { ping: settings.ping, download: settings.download, upload: settings.upload, qos: lossMeasurable() };
  for (const li of $('steps').children) {
    li.classList.remove('active', 'done');
    li.hidden = !on[li.dataset.step];
  }
}

function markStep(phase) {
  const idx = STEP_ORDER.indexOf(phase);
  const at = idx >= 0 ? idx : phase === 'done' ? STEP_ORDER.length : -1;
  if (at < 0) return;
  for (const li of $('steps').children) {
    const i = STEP_ORDER.indexOf(li.dataset.step);
    li.classList.toggle('active', i === at);
    li.classList.toggle('done', i < at && !li.hidden);
  }
}

function message(text, kind = '') {
  const m = $('message');
  m.className = `message ${kind}`;
  m.textContent = text;
  return m;
}

let toastTimer = 0;
function toast(text) {
  const t = $('toast');
  t.textContent = text;
  t.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('show'), 2600);
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

const PHASE_LABEL = {
  connect: 'Connecting',
  greeting: 'Connecting',
  init: 'Preparing',
  pretest: 'Warming up',
  ping: 'Ping',
  download: 'Download',
  upload: 'Upload',
  qos: 'Packet loss',
  save: 'Saving',
  signedresult: 'Signing',
  done: 'Complete',
};
const HALF = { download: 0, upload: 1 };

const state = {
  ready: false,
  running: false,
  logLines: [],
  result: null,      // raw result of the latest run (Details, JSON)
  view: null,        // the run on screen (latest or from history)
  partial: {},
  phase: 'idle',
  prev: null,        // previous run against the same server, for deltas
  loss: false,       // packet loss measurable in this run
  hiddenDuringRun: false,
};

function log(line) {
  const stamp = new Date().toISOString().slice(11, 23);
  state.logLines.push(`${stamp}  ${line}`);
  if (state.logLines.length > 2000) state.logLines.shift();
  if (!$('drawer-details').hidden) renderDetails();
}

const setMode = (mode) => { body.dataset.mode = mode; };

function plannedKeys() {
  const keys = [];
  if (settings.download) keys.push('download');
  if (settings.upload) keys.push('upload');
  if (settings.ping) keys.push('ping');
  if (settings.ping || state.loss) keys.push('jitter');
  if (state.loss) keys.push('loss');
  return keys;
}

function onProgress(p) {
  const phase = p.phase;
  if (phase !== state.phase) enterPhase(phase);
  const half = HALF[phase];

  if (p.final) {
    if (half != null) {
      state.partial[phase] = p.mbps;
      ring.complete(half);
      core.phase.textContent = PHASE_LABEL[phase];
      core.number(p.mbps);
      strip.set(phase, p.mbps);
    } else if (phase === 'ping') {
      if (Array.isArray(p.samplesMs)) ring.setPings(p.samplesMs);
      ring.setRadar(false);
      core.set({ phase: PHASE_LABEL.ping, value: p.pingMs, unit: 'ms' });
      strip.set('ping', p.pingMs);
      strip.set('jitter', p.jitterMs);
    } else if (phase === 'qos') {
      state.partial.loss = p.packetLossPercent;
      strip.set('jitter', p.jitterMs);
      strip.set('loss', p.packetLossPercent);
    }
    return;
  }

  if (half != null) {
    const duration = (half ? settings.uploadS : settings.downloadS) * 1000;
    const elapsed = p.elapsedMs || 0;
    const t = elapsed / duration;
    core.phase.textContent = `${PHASE_LABEL[phase]} · ${(elapsed / 1000).toFixed(1)} s`;
    if (elapsed > 250 && ok(p.mbps)) {
      ring.sample(half, t, p.mbps);
      core.number(p.mbps);
      strip.set(phase, p.mbps, true);
    } else {
      ring.tip = { half, t: Math.min(1, t) };
      ring.kick();
    }
  }
}

function enterPhase(phase) {
  const prev = state.phase;
  state.phase = phase;
  if (HALF[prev] != null) strip.set(prev, state.partial[prev]);
  // The quality phase can still turn out unavailable (endpoint off, UDP
  // blocked); it then reports nothing, so drop its step and the loss item.
  if (prev === 'qos' && !ok(state.partial.loss)) dropLoss();
  markStep(phase);
  const label = PHASE_LABEL[phase] ?? 'Preparing';

  if (HALF[phase] != null) {
    setMode(phase);
    ring.setRadar(false);
    ring.tip = { half: HALF[phase], t: 0 };
    core.set({ phase: label, value: 0, unit: 'Mbit/s' });
  } else if (phase === 'ping') {
    setMode('ping');
    ring.setRadar(true);
    core.set({ phase: label, value: '', unit: 'ms', waiting: true });
  } else if (phase !== 'done') {
    setMode('busy');
    ring.setRadar(true);
    core.set({ phase: label, waiting: true });
  }
}

function dropLoss() {
  state.loss = false;
  const li = document.querySelector('#steps [data-step="qos"]');
  if (li) li.hidden = true;
  if (strip.items.loss) strip.build(plannedKeys().filter((k) => k !== 'loss'), true);
}

async function start() {
  if (!state.ready || state.running) return;
  closeDrawers();
  Object.assign(state, {
    running: true,
    result: null,
    view: null,
    partial: {},
    phase: 'idle',
    logLines: [],
    loss: lossMeasurable(),
    hiddenDuringRun: document.visibilityState !== 'visible',
  });

  body.classList.remove('idle', 'done');
  body.classList.add('busy');
  $('after').hidden = true;
  $('verdict').hidden = true;
  $('path').hidden = true;
  message('');
  strip.el.classList.remove('reveal');
  strip.build(plannedKeys());
  setupSteps();
  ring.clear();
  enterPhase('connect');

  const url = serverUrl();
  state.prev = history().find((e) => e.server === url) || null;
  const options = measurementOptions();
  options.onProgress = onProgress;
  log(`server ${url}`);
  log(`options ${JSON.stringify(options)}`);

  try {
    const r = await run_measurement(url, options, log);
    state.result = r;
    finish(r, url);
    if (legsWanted()) await runLegs(state.view);
  } catch (e) {
    fail(e);
  } finally {
    state.running = false;
    body.classList.remove('busy');
  }
}

/** Ring slots for storage: 3 significant digits, null for empty. */
const compact = (arr) => (arr ? arr.map((v) => (ok(v) ? Number(v.toPrecision(3)) : null)) : null);
const val = (v) => (ok(v) ? v : null);

function finish(r, url) {
  const entry = {
    ts: Date.now(),
    server: url,
    download: settings.download ? val(r.downloadMbps) : null,
    upload: settings.upload ? val(r.uploadMbps) : null,
    ping: val(r.pingMs),
    jitter: val(r.jitterMs),
    loss: val(r.packetLossPercent),
    transport: r.qosTransport,
    threads: r.threads,
    openTestUuid: r.openTestUuid,
    ring: { d: compact(ring.raw[0]), u: compact(ring.raw[1]) },
  };
  markStep('done');
  ring.finish();
  showView(entry, state.prev);
  addHistory(entry);

  const notes = [];
  if (r.downloadError) notes.push(`Download: ${r.downloadError}.`);
  if (r.uploadError) notes.push(`Upload: ${r.uploadError}.`);
  if (state.hiddenDuringRun) notes.push('The tab was in the background during the test, which can lower the results.');
  if (r.openTestUuid) notes.push(`Saved as ${r.openTestUuid}.`);
  message(notes.join(' '), r.downloadError || r.uploadError ? 'error' : '');

  $('announce').textContent = `Measurement complete. ${summaryLine(entry)}`;
  if (fx()) navigator.vibrate?.(14);
}

/** Put a finished run on screen: centre number, strip, verdict, actions. */
function showView(e, prev) {
  state.view = e;
  setMode('done');
  body.classList.remove('idle');
  body.classList.add('done');

  const head = ok(e.download) ? 'download' : ok(e.upload) ? 'upload' : null;
  core.set({
    phase: head ? PHASE_LABEL[head] : ok(e.ping) ? PHASE_LABEL.ping : PHASE_LABEL.done,
    value: head ? e[head] : ok(e.ping) ? e.ping : '✓',
    unit: head ? 'Mbit/s' : ok(e.ping) ? 'ms' : '',
    cta: noHover ? 'Tap to run again' : 'Run again · Space',
    button: 'Run the measurement again',
  });

  // Only what was actually measured: 2–5 items, the grid keeps them even.
  const keys = METRICS.filter((m) => ok(e[m.k])).map((m) => m.k);
  strip.build(keys);
  for (const k of keys) {
    strip.set(k, e[k]);
    if (prev) strip.delta(k, e[k], prev[k]);
  }
  restartAnimation(strip.el, 'reveal');

  renderVerdict(e);
  legsView.show(e);
  $('after').hidden = false;
  restartAnimation($('after'), 'reveal');
  ring.canvas.setAttribute('aria-label', summaryLine(e));
}

function restartAnimation(el, cls) {
  el.classList.remove(cls);
  void el.offsetWidth;
  el.classList.add(cls);
}

function fail(e) {
  const text = typeof e === 'string' ? e : e?.message || String(e);
  log(`error: ${text}`);
  setMode('idle');
  ring.finish();
  ring.flash = 0;
  body.classList.add('idle');
  core.set({
    phase: 'Stopped',
    value: 'Retry',
    cta: noHover ? 'Tap to retry' : 'or press Space',
    button: 'Retry the measurement',
  });
  for (const li of $('steps').children) li.classList.remove('active');
  strip.build([]);
  const friendly = /could not connect|socket closed|connect failed/i.test(text)
    ? 'Could not reach the Nettest server.'
    : 'The measurement stopped.';
  const m = message(`${friendly} ${text}`, 'error');
  const b = document.createElement('button');
  b.textContent = 'Details';
  b.addEventListener('click', () => openDrawer('details'));
  m.append(' ', b);
}

// ---------------------------------------------------------------------------
// Verdict: one plain sentence about what the connection is good for
// ---------------------------------------------------------------------------

/** Thresholds, in one place. Mbit/s and ms. */
const NEEDS = { uhdStream: 25, hdStream: 5, sdStream: 1.5, callDown: 4, callUp: 3, callPing: 150, gamePing: 30, gameJitter: 10, casualPing: 80, backupUp: 100 };

/**
 * What the device gets from the internet: the direct leg when measured,
 * else the slower of the two parts (ping adds up). Without legs: the test.
 */
function effective(e) {
  const wan = e.legs?.wan;
  if (!wan || wan.error) return e;
  const d = e.legs.direct && !e.legs.direct.error ? e.legs.direct : {};
  const lower = (a, b) => (ok(a) && ok(b) ? Math.min(a, b) : ok(a) ? a : b);
  return {
    download: ok(d.download) ? d.download : lower(e.download, wan.download),
    upload: ok(d.upload) ? d.upload : lower(e.upload, wan.upload),
    ping: ok(d.ping) ? d.ping : ok(e.ping) && ok(wan.ping) ? e.ping + wan.ping : wan.ping,
    jitter: e.jitter,
  };
}

function verdictParts(e) {
  const { download: down, upload: up, ping, jitter } = effective(e);
  const good = [];
  if (ok(down)) {
    const n = Math.floor(down / NEEDS.uhdStream);
    if (n >= 100) good.push('over 100 simultaneous 4K streams');
    else if (n > 1) good.push(`about ${n} simultaneous 4K streams`);
    else if (n === 1) good.push('a 4K stream');
    else if (down >= NEEDS.hdStream) good.push('HD streaming');
    else if (down >= NEEDS.sdStream) good.push('SD video and browsing');
  }
  const calls = ok(down) && ok(up) && ok(ping)
    ? down >= NEEDS.callDown && up >= NEEDS.callUp && ping <= NEEDS.callPing
    : null;
  if (calls) good.push('video calls');
  if (ok(ping)) {
    if (ping <= NEEDS.gamePing && (!ok(jitter) || jitter <= NEEDS.gameJitter)) good.push('competitive gaming');
    else if (ping <= NEEDS.casualPing) good.push('casual gaming');
  }
  if (ok(up) && up >= NEEDS.backupUp) good.push('fast cloud backups');
  return { good, calls };
}

function verdictText(e) {
  const { good, calls } = verdictParts(e);
  const join = (a) => (a.length < 2 ? a.join('') : `${a.slice(0, -1).join(', ')} and ${a[a.length - 1]}`);
  let s = good.length ? `Enough for ${join(good)}.` : '';
  if (calls === false) s += `${s ? ' ' : ''}Video calls may stutter.`;
  return { s, good };
}

function renderVerdict(e) {
  const { s, good } = verdictText(e);
  const el = $('verdict');
  el.textContent = '';
  // Bold the capabilities, keep the glue text muted.
  let rest = s;
  for (const g of good) {
    const i = rest.indexOf(g);
    el.append(rest.slice(0, i));
    const b = document.createElement('b');
    b.textContent = g;
    el.append(b);
    rest = rest.slice(i + g.length);
  }
  el.append(rest);
  el.hidden = !s;
  if (s) restartAnimation(el, 'reveal');
}

function summaryLine(e) {
  return METRICS.filter((m) => ok(e[m.k])).map((m) => `${m.label} ${m.fmt(e[m.k])} ${m.unit}`).join(', ');
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

function history() {
  const h = readJson(STORE_HISTORY, []);
  return Array.isArray(h) ? h : [];
}

function addHistory(entry) {
  const h = history();
  h.unshift(entry);
  writeJson(STORE_HISTORY, h.slice(0, HISTORY_MAX));
}

/** Ring halves for a stored run; runs saved before rings existed get a plain one. */
function entryHalves(e) {
  const half = (vals, v) => (Array.isArray(vals) ? normalise(vals) : ok(v) ? new Array(BARS).fill(0.7) : null);
  return [half(e.ring?.d, e.download), half(e.ring?.u, e.upload)];
}

function renderHistory() {
  const h = history();
  const list = $('history-list');
  list.textContent = '';
  $('history-empty').hidden = h.length > 0;
  $('btn-clear-history').hidden = h.length === 0;
  const colors = readColors();
  const dpr = Math.min(2, window.devicePixelRatio || 1);

  h.forEach((e, i) => {
    const li = document.createElement('li');
    const open = document.createElement('button');
    open.className = 'open';
    open.setAttribute('aria-label', `Show measurement from ${fmtTime(e.ts)}`);

    const cv = document.createElement('canvas');
    cv.width = cv.height = 52 * dpr;
    const ctx = cv.getContext('2d');
    ctx.scale(dpr, dpr);
    paintRing(ctx, 26, 26, 21, { colors, halves: entryHalves(e), track: false, glow: false });

    const txt = document.createElement('span');
    txt.className = 'txt';
    const time = document.createElement('time');
    time.dateTime = new Date(e.ts).toISOString();
    time.textContent = fmtTime(e.ts);
    const vals = document.createElement('span');
    vals.className = 'vals';
    const part = (cls, v, unit) => {
      if (!ok(v)) return;
      const s = document.createElement('span');
      s.className = cls;
      const b = document.createElement('b');
      b.textContent = fmtNum(v);
      const small = document.createElement('small');
      small.textContent = ` ${unit}`;
      s.append(b, small);
      vals.append(s);
    };
    part('d', e.download, '↓');
    part('u', e.upload, '↑ Mbit/s');
    part('p', e.ping, 'ms');
    txt.append(time, vals);
    open.append(cv, txt);
    open.addEventListener('click', () => {
      closeDrawers();
      if (state.running) return;
      const prev = h.slice(i + 1).find((x) => x.server === e.server);
      ring.load(e);
      state.result = i === 0 ? state.result : null;
      showView(e, prev);
      message(i === 0 ? '' : `Measured ${fmtTime(e.ts)}.`);
    });

    const del = document.createElement('button');
    del.className = 'icon-btn del';
    del.setAttribute('aria-label', 'Delete this measurement');
    del.innerHTML = '<svg viewBox="0 0 24 24"><use href="#i-trash"/></svg>';
    del.addEventListener('click', () => {
      const all = history();
      all.splice(i, 1);
      writeJson(STORE_HISTORY, all);
      renderHistory();
    });
    li.append(open, del);
    list.append(li);
  });
}

// ---------------------------------------------------------------------------
// Share image: the ring plus the numbers, drawn to a PNG (no library).
// Downloaded via a blob: link, which also works on plain-HTTP pages.
// ---------------------------------------------------------------------------

/** Draw the share card for run `e` and resolve with a PNG blob. */
function renderImage(e) {
  const W = 1200, H = 630;
  const cv = document.createElement('canvas');
  cv.width = W;
  cv.height = H;
  const ctx = cv.getContext('2d');
  const c = readColors();
  const cs = getComputedStyle(document.documentElement);
  const sans = cs.getPropertyValue('--font').trim();
  const mono = cs.getPropertyValue('--mono').trim();

  ctx.fillStyle = c.bg;
  ctx.fillRect(0, 0, W, H);
  const glow = (x, y, r, col, a) => {
    const g = ctx.createRadialGradient(x, y, 0, x, y, r);
    g.addColorStop(0, hexAlpha(col, a));
    g.addColorStop(1, hexAlpha(col, 0));
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, W, H);
  };
  glow(300, 300, 460, c.d2, 0.16);
  glow(1100, 640, 520, c.u1, 0.12);
  ctx.fillStyle = hexAlpha(c.ink, 0.07);
  for (let x = 12; x < W; x += 24) for (let y = 12; y < H; y += 24) ctx.fillRect(x, y, 1.5, 1.5);

  const cx = 320, cy = 315;
  paintRing(ctx, cx, cy, 225, { colors: c, halves: entryHalves(e) });

  const head = ok(e.download) ? 'download' : ok(e.upload) ? 'upload' : 'ping';
  const m = METRICS.find((x) => x.k === head);
  ctx.textAlign = 'center';
  ctx.fillStyle = c.muted;
  ctx.font = `500 14px ${mono}`;
  ctx.fillText(m.label.toUpperCase().split('').join(' '), cx, cy - 58);
  ctx.fillStyle = c.ink;
  ctx.font = `200 92px ${sans}`;
  ctx.fillText(m.fmt(e[head]), cx, cy + 30);
  ctx.fillStyle = c.muted;
  ctx.font = `16px ${mono}`;
  ctx.fillText(m.unit, cx, cy + 64);

  const x0 = 640;
  ctx.textAlign = 'left';
  ctx.fillStyle = c.ink;
  // The nettest.com mark, same geometry as the #logo symbol (180-unit box).
  ctx.save();
  ctx.translate(x0, 72);
  ctx.scale(26 / 180, 26 / 180);
  const lg = ctx.createLinearGradient(70, 30, 170, 180);
  lg.addColorStop(0, '#00abb1');
  lg.addColorStop(1, '#0668ba');
  ctx.fillStyle = lg;
  ctx.fill(new Path2D('M0 0H180V180H0Z M24 24V156H156V24Z M156 97L180 62V101H156Z'), 'evenodd');
  ctx.restore();
  ctx.fillStyle = c.ink;
  ctx.font = `600 20px ${mono}`;
  ctx.fillText('nettest', x0 + 38, 92);
  ctx.fillStyle = c.muted;
  ctx.font = `14px ${mono}`;
  ctx.fillText(new Date(e.ts).toLocaleString(), x0, 120);

  const metric = (mm, x, y, size) => {
    ctx.fillStyle = mm.k === 'download' ? c.d1 : mm.k === 'upload' ? c.u2 : c.faint;
    ctx.beginPath();
    ctx.arc(x + 4, y - 5, 4, 0, TAU);
    ctx.fill();
    ctx.fillStyle = c.muted;
    ctx.font = `500 13px ${mono}`;
    ctx.fillText(mm.label.toUpperCase(), x + 16, y);
    ctx.fillStyle = c.ink;
    ctx.font = `250 ${size}px ${sans}`;
    const v = mm.fmt(e[mm.k]);
    ctx.fillText(v, x, y + size + 8);
    const w = ctx.measureText(v).width;
    ctx.fillStyle = c.muted;
    ctx.font = `15px ${mono}`;
    ctx.fillText(mm.unit, x + w + 10, y + size + 8);
  };
  const big = METRICS.filter((mm) => mm.big && ok(e[mm.k]));
  const small = METRICS.filter((mm) => !mm.big && ok(e[mm.k]));
  big.forEach((mm, i) => metric(mm, x0 + i * 270, 200, 58));
  small.forEach((mm, i) => metric(mm, x0 + i * 180, 330, 38));

  // Verdict, wrapped.
  ctx.fillStyle = c.muted;
  ctx.font = `17px ${sans}`;
  let line = '', y = 452;
  for (const word of verdictText(e).s.split(' ')) {
    const next = line ? `${line} ${word}` : word;
    if (ctx.measureText(next).width > 500 && line) {
      ctx.fillText(line, x0, y);
      line = word;
      y += 26;
    } else line = next;
  }
  if (line) ctx.fillText(line, x0, y);

  ctx.fillStyle = c.faint;
  ctx.font = `13px ${mono}`;
  let host = e.server;
  try { host = new URL(e.server).host; } catch { /* keep */ }
  ctx.fillText(`server ${host}`, x0, 566);

  return new Promise((resolve) => cv.toBlob(resolve, 'image/png'));
}

const imageName = (e) => `nettest-${new Date(e.ts).toISOString().slice(0, 19).replace(/[:T]/g, '-')}.png`;

/**
 * Share needs the Web Share API with files, which browsers only offer on
 * secure pages (HTTPS or localhost) and not on every desktop. Where it is
 * missing (e.g. a phone on http://<LAN-IP>) the button saves the image.
 */
const canShareFiles = (() => {
  try {
    const probe = new File([''], 'x.png', { type: 'image/png' });
    return !!(navigator.canShare && navigator.canShare({ files: [probe] }));
  } catch {
    return false;
  }
})();

function setupImageButton() {
  $('btn-image-label').textContent = canShareFiles ? 'Share' : 'Save image';
  $('btn-image-icon').setAttribute('href', canShareFiles ? '#i-share' : '#i-image');
  $('btn-image').title = canShareFiles ? 'Share the result image' : 'Download the result image';
}

function downloadBlob(blob, name) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

async function shareImage() {
  const e = state.view;
  if (!e) return;
  const blob = await renderImage(e);
  if (!blob) return toast('Could not create the image');
  const file = new File([blob], imageName(e), { type: 'image/png' });
  if (canShareFiles && navigator.canShare({ files: [file] })) {
    try {
      await navigator.share({ files: [file], title: 'Nettest result', text: summaryLine(e) });
      return;
    } catch (err) {
      if (err?.name === 'AbortError') return; // user closed the share sheet
      log(`share failed: ${err?.message || err}`);
    }
  }
  downloadBlob(blob, file.name);
  toast('Image saved');
}

// ---------------------------------------------------------------------------
// Where the speed goes: device <-> server (the test above), server <->
// internet (the server measures itself, /api/upstream) and device <->
// internet (this page against the same public server, as a cross-check).
// Speeds do not add up: end to end should be close to the slower part.
// Ping does: end to end ≈ local + internet.
// ---------------------------------------------------------------------------

let serverInfo = null;

/** The server can measure its own uplink, and we measure that server. */
const legsWanted = () => !!serverInfo?.upstream && settings.internet && !settings.server.trim();

const hopName = (e) => (e?.legs?.gateway ?? serverInfo?.gateway ? 'router' : 'server');
const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1);

const LEGS = [
  { k: 'lan', icon: 'i-device', label: (e) => `This device ↔ ${hopName(e)}` },
  { k: 'wan', icon: 'i-server', label: (e) => `${cap(hopName(e))} ↔ internet` },
  { k: 'direct', icon: 'i-globe', label: () => 'This device ↔ internet' },
];

function legSummary(v) {
  const parts = [];
  if (ok(v.download)) parts.push(`↓ ${fmtNum(v.download)}`);
  if (ok(v.upload)) parts.push(`↑ ${fmtNum(v.upload)}`);
  let s = parts.length ? `${parts.join(' ')} Mbit/s` : '';
  if (ok(v.ping)) s += `${s ? ' · ' : ''}${fmtNum(v.ping)} ms`;
  return s;
}

const legsView = {
  rows: {},
  values: {},

  build(e) {
    const box = $('legs');
    box.textContent = '';
    this.rows = {};
    this.values = {};
    for (const leg of LEGS) {
      const el = document.createElement('div');
      el.className = 'leg waiting';
      el.dataset.leg = leg.k;
      const name = document.createElement('span');
      name.className = 'leg-name';
      name.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true"><use href="#${leg.icon}"/></svg>`;
      const label = document.createElement('span');
      label.textContent = leg.label(e);
      name.append(label);
      const bars = document.createElement('span');
      bars.className = 'bars';
      bars.setAttribute('aria-hidden', 'true');
      bars.innerHTML = '<i class="dn"></i><i class="up"></i>';
      const vals = document.createElement('span');
      vals.className = 'leg-vals';
      vals.textContent = 'waiting';
      el.append(name, bars, vals);
      box.append(el);
      this.rows[leg.k] = { el, vals, dn: bars.children[0], up: bars.children[1] };
    }
    $('path-verdict').textContent = '';
    $('path').hidden = false;
  },

  /** `v`: {download, upload, ping, state, text, error, limit}. */
  set(k, v) {
    const row = this.rows[k];
    if (!row) return;
    this.values[k] = v;
    const st = v.error ? 'failed' : v.state || 'done';
    row.el.className = `leg ${st}${v.limit ? ' limit' : ''}`;
    row.dn.classList.toggle('limit', v.limit === 'download');
    row.vals.textContent = '';
    if (v.error) {
      row.vals.textContent = v.error;
    } else {
      const add = (cls, text) => {
        const s = document.createElement('span');
        if (cls) s.className = cls;
        s.textContent = text;
        row.vals.append(s);
      };
      const num = (val) => {
        const b = document.createElement('b');
        b.textContent = fmtNum(val);
        row.vals.append(b);
      };
      if (ok(v.download)) { add('dn', '↓ '); num(v.download); add('', ' '); }
      if (ok(v.upload)) { add('up', '↑ '); num(v.upload); }
      if (ok(v.download) || ok(v.upload)) add('', ' Mbit/s');
      if (ok(v.ping)) { add('', (ok(v.download) || ok(v.upload)) ? ' · ' : ''); num(v.ping); add('', ' ms'); }
      if (v.text) add('', `${row.vals.childNodes.length ? ' · ' : ''}${v.text}`);
      if (!row.vals.childNodes.length) row.vals.textContent = st === 'waiting' ? 'waiting' : '–';
    }
    this.scale();
  },

  /** One scale for all bars, so the legs compare at a glance. */
  scale() {
    let max = 0;
    for (const v of Object.values(this.values)) {
      if (ok(v.download)) max = Math.max(max, v.download);
      if (ok(v.upload)) max = Math.max(max, v.upload);
    }
    for (const [k, row] of Object.entries(this.rows)) {
      const v = this.values[k] || {};
      const w = (x) => (ok(x) && max > 0 ? Math.max(0.015, x / max).toFixed(4) : '0');
      row.dn.style.setProperty('--w', w(v.download));
      row.up.style.setProperty('--w', w(v.upload));
    }
  },

  /** A finished run, live or from history: only if its legs were measured. */
  show(e) {
    if (!e.legs) {
      $('path').hidden = true;
      return;
    }
    this.build(e);
    this.set('lan', { download: e.download, upload: e.upload, ping: e.ping });
    for (const k of ['wan', 'direct']) {
      const v = e.legs[k];
      this.set(k, v ? { ...v, text: v.error ? '' : v.target ? `via ${v.target}` : '' } : { error: 'not measured' });
    }
    renderPathVerdict(e);
  },
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function api(url, method = 'GET') {
  const res = await fetch(url, { method, cache: 'no-store', headers: { 'X-Nettest': '1' } });
  let data = {};
  try { data = await res.json(); } catch { /* empty or not JSON */ }
  return { status: res.status, data };
}

const UPSTREAM_PHASE = {
  select: 'finding the nearest server', idle: 'starting', greeting: 'connecting', init: 'preparing',
  ping: 'ping', jitter: 'jitter', packetloss: 'packet loss', download: 'download', upload: 'upload',
};

/** Ask the server to measure its own internet connection and follow it. */
async function runUpstream() {
  legsView.set('wan', { state: 'running', text: 'starting' });
  let start;
  try {
    start = await api('api/upstream/start', 'POST');
  } catch (e) {
    return { error: `could not reach the server (${e?.message || e})` };
  }
  const { status, data } = start;
  // A run finished moments ago: reuse it instead of loading the line again.
  if (status === 429) {
    if (data.last?.result) return { ...data.last, reused: true };
    return { error: `measured moments ago, try again in ${data.retryAfterS ?? 60} s` };
  }
  if (status !== 202 && status !== 409) return { error: data.error || `HTTP ${status}` };
  let id = data.id ?? null;
  const deadline = Date.now() + 200_000;
  while (Date.now() < deadline) {
    await sleep(500);
    let s;
    try { s = (await api('api/upstream/status')).data; } catch { continue; }
    if (s.running && s.run) {
      id ??= s.run.id;
      const r = s.run;
      const live = { ...r };
      if (r.phase === 'download' && ok(r.mbps)) live.downloadMbps = r.mbps;
      if (r.phase === 'upload' && ok(r.mbps)) live.uploadMbps = r.mbps;
      const where = r.target ? ` · ${r.target.name}` : '';
      legsView.set('wan', {
        state: 'running',
        download: live.downloadMbps, upload: live.uploadMbps, ping: live.pingMs,
        text: `${UPSTREAM_PHASE[r.phase] ?? r.phase}${where}`,
      });
      continue;
    }
    if (s.last && (id == null || s.last.id >= id)) return s.last;
  }
  return { error: 'the server did not finish its measurement' };
}

const hostPart = (h) => (h.includes(':') && !h.startsWith('[') ? `[${h}]` : h);

function withTimeout(promise, ms, what) {
  let t;
  return Promise.race([
    promise,
    new Promise((_, reject) => { t = setTimeout(() => reject(new Error(`${what} timed out`)), ms); }),
  ]).finally(() => clearTimeout(t));
}

/** This page against the same public server the router used. */
async function runDirect(target) {
  const urls = [];
  // A secure page may not open ws:// (mixed content).
  if (target.wsPort && location.protocol !== 'https:') urls.push(`ws://${hostPart(target.host)}:${target.wsPort}/`);
  if (target.wssPort) urls.push(`wss://${hostPart(target.host)}:${target.wssPort}/`);
  if (!urls.length) return { error: 'the public server has no browser port' };
  let lastError = '';
  for (const url of urls) {
    const live = {};
    let measured = false;
    const onProgress = (p) => {
      if (HALF[p.phase] != null && ok(p.mbps)) { live[p.phase] = p.mbps; measured = true; }
      if (p.phase === 'ping' && p.final) { live.ping = p.pingMs; measured = true; }
      legsView.set('direct', {
        state: 'running', download: live.download, upload: live.upload, ping: live.ping,
        text: PHASE_LABEL[p.phase]?.toLowerCase() ?? p.phase,
      });
    };
    const options = { ...measurementOptions(), qos: false, save: false, signedResult: false, onProgress };
    log(`[internet] server ${url}`);
    try {
      const r = await withTimeout(run_measurement(url, options, (l) => log(`[internet] ${l}`)), 120_000, 'the direct measurement');
      if (ok(r.downloadMbps) || ok(r.uploadMbps)) {
        return { download: val(r.downloadMbps), upload: val(r.uploadMbps), ping: val(r.pingMs), url };
      }
      lastError = r.downloadError || r.uploadError || 'no data transferred';
    } catch (e) {
      lastError = e?.message || String(e);
      log(`[internet] error: ${lastError}`);
    }
    if (measured) break; // it connected and then failed: another port will not help
  }
  return { error: /connect|closed|refused|failed/i.test(lastError) ? 'could not reach the public server from this browser' : lastError };
}

/** Plain values for history and the verdict. */
function legFrom(outcome) {
  if (!outcome || outcome.error || !outcome.result) {
    return { error: outcome?.error || 'not measured', target: outcome?.target?.name };
  }
  const r = outcome.result;
  return {
    download: val(r.downloadMbps), upload: val(r.uploadMbps), ping: val(r.pingMs),
    target: outcome.target?.name, host: outcome.target?.host,
    busy: !!r.busy, cpuLimited: !!r.cpuLimited, cpuPeak: val(r.cpuPeakPercent),
    ageS: outcome.reused ? outcome.ageS : undefined,
  };
}

async function runLegs(e) {
  core.btn.disabled = true;
  core.cta.textContent = 'Checking the internet side…';
  e.legs = { gateway: !!serverInfo?.gateway };
  legsView.build(e);
  legsView.set('lan', { download: e.download, upload: e.upload, ping: e.ping });

  const up = await runUpstream();
  e.legs.wan = legFrom(up);
  legsView.set('wan', { ...e.legs.wan, text: e.legs.wan.target && !e.legs.wan.error ? `via ${e.legs.wan.target}` : '' });

  if (up.target && (up.target.wsPort || up.target.wssPort)) {
    legsView.set('direct', { state: 'running', text: `connecting to ${up.target.name}` });
    e.legs.direct = { ...(await runDirect(up.target)), target: up.target.name };
  } else {
    e.legs.direct = { error: 'skipped: no public server' };
  }
  delete e.legs.direct.url;
  legsView.show(e);

  // Keep the legs with the run in the history.
  const h = history();
  const i = h.findIndex((x) => x.ts === e.ts);
  if (i >= 0) { h[i] = e; writeJson(STORE_HISTORY, h); }

  core.btn.disabled = false;
  core.cta.textContent = noHover ? 'Tap to run again' : 'Run again · Space';
  renderVerdict(e); // now based on what reaches the internet
  const pv = pathVerdict(e);
  if (pv) $('announce').textContent = pv.text;
}

/** One sentence on where the bottleneck is, plus notes. Download first. */
function pathVerdict(e) {
  const lan = { download: e.download, upload: e.upload, ping: e.ping };
  const wan = e.legs?.wan;
  const direct = e.legs?.direct;
  if (!wan || wan.error) return null;
  const hop = hopName(e);
  const local = hop === 'router' ? 'Wi-Fi or local network' : `link to the ${hop}`;
  const dir = ok(lan.download) && ok(wan.download) ? 'download' : ok(lan.upload) && ok(wan.upload) ? 'upload' : null;
  const parts = []; // [text, bold?]
  const notes = [];
  let limit = null;
  if (dir) {
    const a = lan[dir];
    const b = wan[dir];
    if (a < b * 0.8) {
      limit = 'lan';
      parts.push(['Your ', false], [local, true], [` is the bottleneck: ${fmtNum(a)} of the ${fmtNum(b)} Mbit/s ${dir} your internet line delivers reach this device.`, false]);
    } else if (b < a * 0.8) {
      limit = 'wan';
      parts.push(['Your ', false], ['internet connection', true], [` is the bottleneck: the ${hop} gets ${fmtNum(b)} Mbit/s ${dir}, this device could take ${fmtNum(a)}.`, false]);
    } else {
      parts.push(['Your local network and internet connection are ', false], ['well matched', true], [` at about ${fmtNum(Math.min(a, b))} Mbit/s ${dir}.`, false]);
    }
    const d = direct && !direct.error ? direct[dir] : null;
    if (ok(d)) {
      const expected = Math.min(a, b);
      if (wan.cpuLimited && d > b * 1.15) {
        notes.push(`The ${hop}'s processor was saturated while measuring its own line; the direct result (${fmtNum(d)} Mbit/s) is the better figure for your internet speed.`);
      } else if (d < expected * 0.7) {
        parts.push([` Through the ${hop} you get only ${fmtNum(d)} Mbit/s, less than either part: the `, false], [`${hop} itself`, true], [' may be slowing traffic down.', false]);
      } else {
        notes.push(`Cross-check: ${fmtNum(d)} Mbit/s from this device straight to the internet, consistent with the two parts.`);
      }
    }
  }
  if (ok(lan.ping) && ok(wan.ping) && direct && ok(direct.ping)) {
    notes.push(`Ping: ${fmtNum(lan.ping)} ms local + ${fmtNum(wan.ping)} ms internet ≈ ${fmtNum(direct.ping)} ms end to end.`);
  }
  if (wan.busy) notes.push('Other devices were using the internet during the test, so the internet figures may be low.');
  if (ok(wan.ageS) && wan.ageS > 0) notes.push(`The ${hop}'s result is from ${Math.max(1, Math.round(wan.ageS / 60))} min ago.`);
  const text = [...parts.map((p) => p[0]), ...notes.map((n) => ` ${n}`)].join('').trim();
  return { parts, notes, limit, text };
}

function renderPathVerdict(e) {
  const el = $('path-verdict');
  el.textContent = '';
  const pv = pathVerdict(e);
  if (!pv) return;
  for (const [t, bold] of pv.parts) {
    if (bold) {
      const b = document.createElement('b');
      b.textContent = t;
      el.append(b);
    } else el.append(t);
  }
  if (pv.notes.length) {
    const s = document.createElement('small');
    s.textContent = pv.notes.join(' ');
    el.append(s);
  }
  if (pv.limit) {
    const row = legsView.rows[pv.limit];
    if (row) row.el.classList.add('limit');
    row?.dn.classList.add('limit');
  }
}

// ---------------------------------------------------------------------------
// Details, copy
// ---------------------------------------------------------------------------

function renderDetails() {
  const r = state.result;
  const kv = $('details-kv');
  kv.textContent = '';
  const rows = [
    ['Server', serverUrl()],
    ['Page', `${location.protocol}//${location.host} (${window.isSecureContext ? 'secure' : 'not secure'} context)`],
    ['WebTransport', 'WebTransport' in window ? 'available' : 'not available on this page'],
  ];
  if (r) {
    rows.push(
      ['Download', `${fmtNum(r.downloadMbps)} Mbit/s · ${fmtBytes(r.downloadBytes)}`],
      ['Upload', `${fmtNum(r.uploadMbps)} Mbit/s · ${fmtBytes(r.uploadBytes)}`],
      ['Ping', `${fmtNum(r.pingMs)} ms`],
    );
    if (ok(r.jitterMs)) {
      rows.push(['Jitter', `${fmtNum(r.jitterMs)} ms (${r.qosTransport === 'websocket' ? 'from ping round trips' : r.qosTransport})`]);
    }
    if (ok(r.packetLossPercent)) rows.push(['Packet loss', `${r.packetLossPercent.toFixed(2)} %`]);
    if (r.qos) {
      rows.push(
        ['QoS out', `${r.qos.out.received}/${r.qos.out.sent} · ${fmtNum(r.qos.out.jitterMs)} ms jitter`],
        ['QoS in', `${r.qos.in.received}/${r.qos.in.sent} · ${fmtNum(r.qos.in.jitterMs)} ms jitter`],
      );
    }
    rows.push(['QoS transport', r.qosTransport ?? 'none']);
    rows.push(
      ['Connections', String(r.threads)],
      ['Chunk size', fmtBytes(r.chunkSize)],
      ['Duration', `${(r.durationMs / 1000).toFixed(1)} s`],
    );
    if (r.openTestUuid) rows.push(['Open test UUID', r.openTestUuid]);
    if (r.envelope) rows.push(['Signed result', `${r.envelope.length} bytes (see JSON)`]);
  }
  for (const [k, v] of rows) {
    const dt = document.createElement('dt');
    dt.textContent = k;
    const dd = document.createElement('dd');
    dd.textContent = v;
    kv.append(dt, dd);
  }
  $('log').textContent = state.logLines.join('\n') || 'No measurement yet.';
  $('json').textContent = r ? JSON.stringify(resultJson(r), null, 2) : '–';
}

function resultJson(r) {
  return { time: new Date().toISOString(), server: serverUrl(), userAgent: navigator.userAgent, ...r };
}

function summaryText(e) {
  const lines = [`Nettest — ${new Date(e.ts).toLocaleString()}`];
  for (const m of METRICS) {
    if (ok(e[m.k])) lines.push(`${`${m.label}:`.padEnd(13)}${m.fmt(e[m.k])} ${m.unit}`);
  }
  const { s } = verdictText(e);
  if (s) lines.push(s);
  lines.push(`${'Server:'.padEnd(13)}${e.server}`);
  for (const leg of LEGS.slice(1)) {
    const v = e.legs?.[leg.k];
    if (v && !v.error) lines.push(`${leg.label(e)}: ${legSummary(v)}${v.target ? ` (${v.target})` : ''}`);
  }
  const pv = e.legs && pathVerdict(e);
  if (pv) lines.push(pv.text);
  return lines.join('\n');
}

/** Clipboard that also works on plain-HTTP pages (no Clipboard API there). */
async function copyText(text) {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch { /* fall through */ }
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.setAttribute('readonly', '');
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.append(ta);
  ta.select();
  let done = false;
  try { done = document.execCommand('copy'); } catch { done = false; }
  ta.remove();
  return done;
}

// ---------------------------------------------------------------------------
// Drawers
// ---------------------------------------------------------------------------

let lastFocus = null;

function openDrawer(name) {
  closeDrawers(false);
  const d = $(`drawer-${name}`);
  if (name === 'history') renderHistory();
  if (name === 'details') renderDetails();
  if (name === 'settings') fillSettingsForm();
  lastFocus = document.activeElement;
  $('scrim').hidden = false;
  d.hidden = false;
  d.querySelector('[data-close]').focus();
}

function closeDrawers(restoreFocus = true) {
  let wasOpen = false;
  for (const d of document.querySelectorAll('.drawer')) {
    if (!d.hidden) wasOpen = true;
    d.hidden = true;
  }
  $('scrim').hidden = true;
  if (wasOpen && restoreFocus && lastFocus?.focus) lastFocus.focus();
}

const anyDrawerOpen = () => [...document.querySelectorAll('.drawer')].some((d) => !d.hidden);

// ---------------------------------------------------------------------------
// Settings form
// ---------------------------------------------------------------------------

const form = $('settings-form');

function fillSettingsForm() {
  for (const el of form.elements) {
    if (!el.name || !(el.name in settings)) continue;
    if (el.type === 'checkbox') el.checked = settings[el.name];
    else el.value = settings[el.name];
  }
  form.elements.server.placeholder = serverUrl();
  syncDependentFields();
}

function syncDependentFields() {
  form.elements.controlServer.disabled = !settings.save;
  form.elements.downloadWindowMiB.disabled = !settings.boundedDownload;
  form.elements.qosPackets.disabled = !settings.qos;
  form.elements.qosDelayMs.disabled = !settings.qos;
}

function applyFx() {
  body.classList.toggle('nofx', !settings.fx);
  ring.kick();
}

form.addEventListener('change', (ev) => {
  const el = ev.target;
  if (!el.name || !(el.name in settings)) return;
  settings[el.name] = coerce(el.name, el.type === 'checkbox' ? el.checked : el.value);
  saveSettings();
  syncDependentFields();
  if (!state.running) setupSteps();
  if (el.name === 'server') refreshServerInfo();
  if (el.name === 'fx') applyFx();
});

$('btn-reset-settings').addEventListener('click', () => {
  settings = { ...DEFAULTS };
  saveSettings();
  fillSettingsForm();
  setupSteps();
  refreshServerInfo();
  applyFx();
  toast('Settings reset');
});

$('btn-share-settings').addEventListener('click', async () => {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(settings)) {
    if (v !== DEFAULTS[k]) q.set(k, typeof v === 'boolean' ? (v ? '1' : '0') : String(v));
  }
  const link = `${location.origin}${location.pathname}${q.toString() ? `?${q}` : ''}`;
  toast((await copyText(link)) ? 'Link copied' : link);
});

// ---------------------------------------------------------------------------
// The measured path: this device <-> server
// ---------------------------------------------------------------------------

async function refreshServerInfo() {
  let host = location.host;
  if (settings.server.trim()) {
    try { host = new URL(settings.server.trim()).host; } catch { host = settings.server.trim(); }
  }
  $('link-server').textContent = host || '–';
  $('link').title = `Measuring the path between this device and ${host}`;
  try {
    const res = await fetch('api/info', { cache: 'no-store' });
    if (!res.ok) throw new Error(res.statusText);
    const info = await res.json();
    serverInfo = info;
    $('link-client').textContent = info.clientIp || 'this browser';
    $('link-server-name').textContent = settings.server.trim() ? 'Server' : info.name || 'Nettest server';
    if (info.version) $('foot-version-text').textContent = `nettest ${info.version}`;
  } catch {
    $('link-client').textContent = 'this browser';
  }
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

$('btn-history').addEventListener('click', () => openDrawer('history'));
$('btn-settings').addEventListener('click', () => openDrawer('settings'));
$('btn-details').addEventListener('click', () => openDrawer('details'));
$('btn-image').addEventListener('click', shareImage);
$('scrim').addEventListener('click', () => closeDrawers());
for (const b of document.querySelectorAll('[data-close]')) b.addEventListener('click', () => closeDrawers());

$('btn-copy').addEventListener('click', async () => {
  if (!state.view) return;
  toast((await copyText(summaryText(state.view))) ? 'Results copied' : 'Copy failed');
});

$('btn-download-json').addEventListener('click', () => {
  if (!state.result) return toast('No result yet');
  const blob = new Blob([JSON.stringify(resultJson(state.result), null, 2)], { type: 'application/json' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `nettest-${new Date().toISOString().replace(/[:.]/g, '-')}.json`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
});

$('btn-clear-history').addEventListener('click', () => {
  writeJson(STORE_HISTORY, []);
  renderHistory();
  toast('History cleared');
});

document.addEventListener('keydown', (ev) => {
  if (ev.key === 'Escape') return closeDrawers();
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName);
  if (typing || ev.metaKey || ev.ctrlKey || ev.altKey || anyDrawerOpen()) return;
  if (ev.key === 'h') openDrawer('history');
  else if (ev.key === 's') openDrawer('settings');
  else if ((ev.key === ' ' || ev.key === 'Enter') && !state.running && document.activeElement === body) {
    ev.preventDefault();
    start();
  }
});

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden' && state.running) state.hiddenDuringRun = true;
});
reduceMotion.addEventListener('change', () => ring.kick());

/** Resolve once the page is visible: background tabs throttle timers to ~1/s. */
function whenVisible() {
  if (document.visibilityState === 'visible') return Promise.resolve();
  return new Promise((resolve) => {
    const on = () => {
      if (document.visibilityState === 'visible') {
        document.removeEventListener('visibilitychange', on);
        resolve();
      }
    };
    document.addEventListener('visibilitychange', on);
  });
}

const nextFrame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));

async function boot() {
  body.classList.toggle('nofx', !settings.fx);
  ring.setup();
  setupSteps();
  setupImageButton();
  setMode('idle');
  body.classList.add('idle');
  core.set({ phase: 'Loading', waiting: true });
  const infoLoaded = refreshServerInfo();

  try {
    await init();
    state.ready = true;
  } catch (e) {
    core.set({ phase: 'Unavailable' });
    message(`This browser could not load the measurement engine (WebAssembly): ${e?.message || e}`, 'error');
    return;
  }

  if (settings.autostart) {
    core.set({ phase: 'Ready' });
    await whenVisible();
    await nextFrame(); // let the ring paint before traffic starts
    await infoLoaded; // decides whether the internet legs run
    start();
  } else {
    core.set({
      phase: 'Ready',
      value: 'Start',
      cta: noHover ? 'Tap to begin' : 'or press Space',
      button: 'Start the measurement',
    });
  }
}

boot();
