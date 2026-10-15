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

/**
 * Speeds first: ping, jitter and packet loss run after the result is on
 * screen and fill in as they arrive. Not when saving to a control server,
 * which needs one complete result.
 */
const deferQuality = () => !settings.save;

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
// Smooth the ring between samples while one paint costs less than this.
const SMOOTH_MAX_PAINT_MS = 2.5;
// New bars sweep in over about one progress interval.
const REVEAL_MS = 160;
// Frame interval while the browser itself measures. Bars arrive at ~10/s
// (72 per 7 s), so 10 fps still sweeps one bar per frame; faster costs
// throughput on slow devices (canvas raster shares the main thread).
const TRANSFER_FRAME_MS = 100;
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
  const rIn = R * (o.inner || 0.64);
  const span = R - rIn;
  const step = Math.PI / BARS;
  const w = Math.max(1, rIn * step * 0.52);
  const orbit = o.orbit !== false;
  ctx.lineCap = 'round';

  // Hairline orbit and the two handover marks (12 and 6 o'clock).
  ctx.strokeStyle = c.line;
  ctx.lineWidth = 1;
  if (orbit) {
    ctx.beginPath();
    ctx.arc(cx, cy, R + w * 1.6, 0, TAU);
    ctx.stroke();
  }
  if (orbit && o.track !== false) {
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
  if (o.flash > 0 && orbit) {
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

/**
 * Concentric orbits, inner to outer: [inner, outer] radius as a fraction of
 * R. One band for a plain test; with the internet legs, device ↔ server,
 * server ↔ internet and device ↔ internet.
 */
const BANDS = { 1: [[0.64, 1]], 3: [[0.61, 0.73], [0.76, 0.87], [0.9, 1]] };

/** Paint `bands` (each [down, up] normalised slots); live extras go to `o.active`. */
function paintBands(ctx, cx, cy, R, bands, o) {
  const geom = BANDS[bands.length] || BANDS[1];
  bands.forEach((halves, i) => {
    const [a, b] = geom[i];
    paintRing(ctx, cx, cy, R * b, {
      ...o,
      halves,
      inner: a / b,
      orbit: i === bands.length - 1,
      tip: o.active === i ? o.tip : null,
      // The radar and ping dots belong to the open centre: innermost band.
      radar: i === 0 ? o.radar : null,
      pings: i === 0 ? o.pings : null,
    });
  });
}

/** Raw Mbit/s slots per band for a stored run (flat when only a value is known). */
function entryRaw(e) {
  const src = e.legs ? [[e.ring, e], [e.legs.wan?.ring, e.legs.wan], [e.legs.direct?.ring, e.legs.direct]] : [[e.ring, e]];
  return src.map(([r, v]) => [r?.d, r?.u].map((vals, h) => {
    if (Array.isArray(vals)) return vals.map((x) => (x == null ? NaN : x));
    const x = h ? v?.upload : v?.download;
    return ok(x) ? new Array(BARS).fill(x) : null;
  }));
}

/** One scale per direction across all bands, so the orbits compare. */
function sharedScale(raw) {
  return [0, 1].map((h) => Math.max(0, ...raw.flatMap((b) => (b[h] || []).filter(ok))) * 1.1 || 1);
}

function entryBands(e) {
  const raw = entryRaw(e);
  const s = sharedScale(raw);
  return raw.map((b) => b.map((vals, h) => normalise(vals, s[h])));
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
  n: 1,                     // concentric bands: 1, or 3 with the internet legs
  band: 0,                  // band the live samples go to
  others: [],               // finished bands: [down, up] raw Mbit/s slots
  otherPeak: [0, 0],
  raf: 0,
  last: 0,
  lastDraw: 0,
  frameMs: 0,               // smoothed paint cost
  timer: 0,
  reveal: [-1, -1],         // bars drawn so far per half (eases towards lastIdx)
  idle: false,              // browser not measuring (server leg): full frame rate is free
  measuring: 0,             // browser measurements in flight: pace the frame loop
  hover: null,              // { band, half, idx } bar under the pointer
  tipEl: $('ring-tip'),
  tipTimer: 0,

  /** Animate between samples? Always when the browser is idle, otherwise only while painting stays cheap. */
  smooth() {
    return fx() && (this.idle || this.frameMs < SMOOTH_MAX_PAINT_MS);
  },

  setup() {
    this.ctx = this.canvas.getContext('2d');
    new ResizeObserver(() => this.resize()).observe(this.canvas);
    const point = (ev) => {
      const r = this.canvas.getBoundingClientRect();
      clearTimeout(this.tipTimer);
      this.setHover(this.hit(ev.clientX - r.left, ev.clientY - r.top), ev.clientX - r.left, ev.clientY - r.top);
    };
    this.canvas.addEventListener('pointermove', point);
    this.canvas.addEventListener('pointerdown', point);
    this.canvas.addEventListener('pointerleave', (ev) => {
      // Touch: keep the value readable for a moment after the finger lifts.
      if (ev.pointerType === 'mouse') this.setHover(null);
      else this.tipTimer = setTimeout(() => this.setHover(null), 1800);
    });
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

  /** Band, half and slot of the bar under canvas point (x, y), or null. */
  hit(x, y) {
    const size = this.size;
    if (!size) return null;
    const R = size / 2 - size * 0.03;
    const dx = x - size / 2, dy = y - size / 2;
    const r = Math.hypot(dx, dy) / R;
    const geom = BANDS[this.n] || BANDS[1];
    // Nearest band, with a little slack around it for thin bars.
    let band = -1, best = 0.04;
    geom.forEach(([a, b], i) => {
      const d = r < a ? a - r : r > b ? r - b : 0;
      if (d <= best) { best = d; band = i; }
    });
    if (band < 0) return null;
    let turn = (Math.atan2(dy, dx) - START) / TAU;
    turn -= Math.floor(turn);
    const half = turn < 0.5 ? 0 : 1;
    const idx = Math.min(BARS - 1, Math.floor((turn * 2 - half) * BARS));
    const live = band === this.band;
    if (live && idx > this.reveal[half]) return null; // not swept in yet
    const v = (live ? this.raw : this.others[band] || [])[half]?.[idx];
    return ok(v) ? { band, half, idx, v } : null;
  },

  /** Show the value of the bar under the pointer; null hides it. */
  setHover(h, x = 0, y = 0) {
    const el = this.tipEl;
    const same = h && this.hover && h.band === this.hover.band && h.half === this.hover.half && h.idx === this.hover.idx;
    this.hover = h;
    this.canvas.style.cursor = h ? 'crosshair' : '';
    if (!h) {
      el.classList.remove('on');
      this.draw();
      return;
    }
    el.classList.toggle('below', y < this.size * 0.22);
    if (!same) {
      // The server's own leg always runs 7 s per direction.
      const ms = this.n === 3 && h.band === 1 ? SERVER_LEG_MS : (h.half ? settings.uploadS : settings.downloadS) * 1000;
      const at = ((h.idx + 1) / BARS) * ms / 1000; // end of this slot
      $('ring-tip-value').textContent = fmtNum(h.v);
      const dir = $('ring-tip-dir');
      dir.textContent = h.half ? '↑ Upload' : '↓ Download';
      dir.className = h.half ? 'u' : 'd';
      $('ring-tip-at').textContent = `at ${at.toFixed(1)} s`;
      $('ring-tip-leg').textContent = this.n === 3 ? LEGS[h.band]?.short || '' : '';
      this.draw();
    }
    // Keep the box inside the ring area (and so on screen on phones).
    const half = el.offsetWidth / 2 + 4;
    const cx = Math.min(Math.max(x, half), this.size - half);
    el.style.setProperty('--x', `${Math.round(cx)}px`);
    el.style.setProperty('--y', `${Math.round(y)}px`);
    el.classList.add('on');
  },

  clear(n = 1) {
    this.n = n;
    this.hover = null;
    this.tipEl.classList.remove('on');
    this.band = 0;
    this.others = [];
    this.otherPeak = [0, 0];
    this.resetLive();
    this.scale = [1, 1];
    this.pings = [];
    this.flash = 0;
    this.kick();
  },

  resetLive() {
    this.raw = [null, null];
    for (const s of this.shown) s.fill(NaN);
    this.lastIdx = [-1, -1];
    this.reveal = [-1, -1];
    this.peak = [0, 0];
    this.tip = null;
  },

  /** Park the live band and send samples to band `i` from now on. */
  beginBand(i) {
    if (this.raw[0] || this.raw[1]) this.others[this.band] = this.raw.map((r) => (r ? r.slice() : null));
    this.band = i;
    this.resetLive();
    this.otherPeak = [0, 1].map((h) => Math.max(0, ...this.others.flatMap((b) => (b?.[h] || []).filter(ok))));
    this.kick();
  },

  /** Raw slots of band `i` (live or parked), for storage. */
  bandRaw(i) {
    return i === this.band ? this.raw : this.others[i] || [null, null];
  },

  /** Load a finished ring (history entry), no animation. */
  load(entry) {
    const raw = entryRaw(entry);
    this.clear(raw.length);
    this.others = raw;
    this.band = -1;
    this.otherPeak = [0, 1].map((h) => Math.max(0, ...raw.flatMap((b) => (b[h] || []).filter(ok))));
    this.scale = this.otherPeak.map((p) => p * 1.1 || 1);
    this.draw();
  },

  /** Display scale target per half: one scale across all bands. */
  target(h) {
    return Math.max(this.peak[h], this.otherPeak[h]) * 1.1 || 1;
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

  /** A half known only by its final value (the server's upload): a flat band. */
  fill(half, v) {
    if (!ok(v)) return;
    this.raw[half] = new Array(BARS).fill(v);
    this.lastIdx[half] = BARS - 1;
    this.reveal[half] = BARS - 1;
    this.peak[half] = Math.max(this.peak[half], v);
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
    // During transfers the measurement owns the main thread: unless painting
    // is cheap (or the browser is idle), no frame loop, just one paint per
    // progress sample (every ~150 ms).
    const snap = this.tip && !this.flash && !this.smooth();
    if (snap) {
      this.stopLoop();
      this.pings = [];
      this.radar = false;
    }
    if (!fx() || snap) {
      // No animation: snap everything and paint once.
      for (let h = 0; h < 2; h++) {
        this.scale[h] = this.target(h);
        this.reveal[h] = this.lastIdx[h];
        if (!this.raw[h]) continue;
        const n = normalise(this.raw[h], this.scale[h]);
        for (let i = 0; i < BARS; i++) this.shown[h][i] = n[i];
      }
      for (const p of this.pings) p.alpha = 1;
      this.flash = 0;
      this.timedDraw();
      return;
    }
    if (this.raf) return;
    this.last = performance.now();
    this.raf = requestAnimationFrame((t) => this.tick(t));
  },

  stopLoop() {
    if (this.raf > 0) cancelAnimationFrame(this.raf);
    clearTimeout(this.timer);
    this.raf = 0;
  },

  /** Next frame after `ms`; the loop costs nothing in between. */
  later(ms) {
    this.raf = -1; // pending: kick() must not start a second loop
    this.timer = setTimeout(() => { this.raf = requestAnimationFrame((t) => this.tick(t)); }, ms);
  },

  tick(now) {
    this.raf = 0;
    const dt = Math.min(150, now - this.last);
    this.last = now;
    let moving = this.radar || !!this.tip;

    for (let h = 0; h < 2; h++) {
      const target = this.target(h);
      this.scale[h] += (target - this.scale[h]) * (1 - Math.exp(-dt / 350));
      if (Math.abs(target - this.scale[h]) > target * 0.002) moving = true;
      else this.scale[h] = target;
      // New bars sweep in over ~one sample interval instead of all at once.
      const goal = this.lastIdx[h];
      if (this.reveal[h] < goal) {
        const step = Math.max(dt * (BARS / 7000), (goal - this.reveal[h]) * (1 - Math.exp(-dt / REVEAL_MS)));
        this.reveal[h] = Math.min(goal, this.reveal[h] + step);
        moving = true;
      } else this.reveal[h] = goal;
      const raw = this.raw[h];
      if (!raw) continue;
      const k = 1 - Math.exp(-dt / 120);
      const shown = this.shown[h];
      const limit = this.reveal[h];
      for (let i = 0; i < BARS; i++) {
        const v = raw[i];
        if (!ok(v) || i > limit) { shown[i] = NaN; continue; }
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
    // The big number re-lays out on every change: while the browser measures
    // it only moves with the samples.
    if (!this.tip || this.idle) core.tick(dt) && (moving = true);
    else core.snap();

    this.lastDraw = now;
    this.timedDraw();
    // While the browser measures, the measurement owns the main thread: the
    // loop is paced by a timer (no idle animation frames), and goes back to
    // one paint per sample (kick) as soon as painting gets expensive.
    const transfer = !!this.tip;
    if (transfer && !this.flash && !this.smooth()) {
      this.kick();
      return;
    }
    if (!moving) return;
    // Pretest (which sizes the chunks), ping and transfers all need the main
    // thread: while the browser measures, the loop is paced by a timer.
    if (this.measuring > 0 && !this.idle && !this.flash) this.later(TRANSFER_FRAME_MS - 16);
    else this.raf = requestAnimationFrame((t) => this.tick(t));
  },

  timedDraw() {
    const t0 = performance.now();
    this.draw();
    this.frameMs = this.frameMs * 0.9 + (performance.now() - t0) * 0.1;
  },

  draw() {
    const { ctx, size } = this;
    if (!ctx || !size) return;
    if (!this.colors) this.colors = readColors();
    ctx.clearRect(0, 0, size, size);
    const pad = size * 0.03;
    const bands = [];
    for (let i = 0; i < this.n; i++) {
      bands.push(i === this.band
        ? this.shown
        : (this.others[i] || [null, null]).map((vals, h) => normalise(vals, this.scale[h])));
    }
    paintBands(ctx, size / 2, size / 2, size / 2 - pad, bands, {
      colors: this.colors,
      active: this.band,
      // The tip follows the bars as they sweep in.
      tip: this.tip && { ...this.tip, t: Math.min(this.tip.t, (this.reveal[this.tip.half] + 1) / BARS) },
      radar: this.radar ? this.radarAngle : null,
      pings: this.pings,
      flash: this.flash,
      glow: !this.tip, // the wide glow pass doubles the stroke cost; skip it mid-transfer
    });
    this.drawHover(ctx, size / 2, size / 2, size / 2 - pad);
  },

  /** Outline the bar under the pointer. */
  drawHover(ctx, cx, cy, R) {
    const h = this.hover;
    if (!h) return;
    const [a, b] = (BANDS[this.n] || BANDS[1])[h.band] || [];
    if (a == null) return;
    const rIn = R * a;
    const span = R * b - rIn;
    const w = Math.max(1, rIn * (Math.PI / BARS) * 0.52);
    const len = Math.max(w * 0.4, Math.min(1, h.v / this.scale[h.half]) * span);
    const ang = angleAt(h.half, (h.idx + 0.5) / BARS);
    const cos = Math.cos(ang), sin = Math.sin(ang);
    ctx.lineCap = 'round';
    ctx.strokeStyle = this.colors.ink;
    ctx.lineWidth = w * 1.25;
    ctx.beginPath();
    ctx.moveTo(cx + cos * rIn, cy + sin * rIn);
    ctx.lineTo(cx + cos * (rIn + len), cy + sin * (rIn + len));
    ctx.stroke();
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

  snap() {
    if (!ok(this.target) || this.shown === this.target) return;
    this.shown = this.target;
    const text = this.fmt(this.shown);
    if (this.value.textContent !== text) this.value.textContent = text;
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
const STEP_LABEL = { ping: 'Ping', download: 'Download', upload: 'Upload', qos: 'Quality' };

/** Phases of a plain test, or the three legs when the internet side runs. */
function setupSteps(three = false) {
  const ol = $('steps');
  ol.textContent = '';
  ol.classList.toggle('legs', three);
  const on = { ping: settings.ping, download: settings.download, upload: settings.upload, qos: lossMeasurable() };
  const items = three
    ? LEGS.map((l) => [l.k, l.short])
    : STEP_ORDER.filter((k) => on[k]).map((k) => [k, STEP_LABEL[k]]);
  for (const [k, label] of items) {
    const li = document.createElement('li');
    li.dataset.step = k;
    li.textContent = label;
    ol.append(li);
  }
}

function markStep(phase) {
  if (state.three) return; // the legs are marked by markLeg()
  const idx = STEP_ORDER.indexOf(phase);
  const at = idx >= 0 ? idx : phase === 'done' ? STEP_ORDER.length : -1;
  if (at < 0) return;
  for (const li of $('steps').children) {
    const i = STEP_ORDER.indexOf(li.dataset.step);
    li.classList.toggle('active', i === at);
    li.classList.toggle('done', i < at);
  }
}

function markLeg(at) {
  [...$('steps').children].forEach((li, i) => {
    li.classList.toggle('active', i === at);
    li.classList.toggle('done', i < at);
  });
}

// ---------------------------------------------------------------------------
// Topology header: This device ── Server ── Internet, plus the end-to-end
// arc (under the server on a LAN, straight through it on a gateway).
// ---------------------------------------------------------------------------

const topo = {
  el: $('link'),
  wires() { return [...this.el.querySelectorAll('[data-w]')]; },
  wire(k) { return this.el.querySelector(`[data-w="${k}"]`); },

  serverName: null,
  serverHost: null,

  /** `net`: this device measures the internet directly (named public server). */
  setup(three, relation, net = null) {
    $('link-server-name').textContent = net ? 'Internet' : this.serverName ?? $('link-server-name').textContent;
    $('link-server').textContent = net ?? this.serverHost ?? $('link-server').textContent;
    this.el.classList.toggle('three', three);
    this.el.classList.toggle('thru', three && relation === 'gateway');
    for (const w of this.wires()) {
      w.className = w.className.replace(/\b(on|done|limit|failed)\b/g, '').trim();
      w.querySelector('em').textContent = '';
    }
    if (!three) this.wire('lan').classList.add('on');
    $('link-net').textContent = three ? 'nearest server' : '–';
  },

  /** Animate the segment(s) being measured; null for none. */
  active(k) {
    const thru = this.el.classList.contains('thru');
    for (const w of this.wires()) {
      const on = w.dataset.w === k || (thru && k === 'direct' && w.dataset.w !== 'direct');
      w.classList.toggle('on', on);
    }
  },

  /** Values under a finished segment. */
  value(k, v, limit = false) {
    const w = this.wire(k);
    if (!w) return;
    w.classList.remove('on');
    w.classList.toggle('done', !v?.error);
    w.classList.toggle('failed', !!v?.error);
    w.classList.toggle('limit', limit);
    const em = w.querySelector('em');
    em.textContent = '';
    if (v?.error) em.textContent = 'not measured';
    else {
      // Upload and ping are secondary: narrow screens hide them (CSS).
      const add = (text, cls) => {
        const s = document.createElement('span');
        if (cls) s.className = cls;
        s.textContent = text;
        em.append(s);
      };
      if (ok(v?.download)) add(`↓${fmtNum(v.download)}`);
      if (ok(v?.upload)) add(` ↑${fmtNum(v.upload)}`, 'sec');
      if (k === 'direct' && ok(v?.ping)) add(` · ${fmtNum(v.ping)} ms`, 'sec');
    }
  },

  show(e) {
    const three = !!e.legs;
    this.setup(three, e.legs?.relation, e.via ?? null);
    if (!three) return;
    const pv = pathVerdict(e);
    const lim = pv?.limit?.leg;
    this.value('lan', { download: e.download, upload: e.upload }, lim === 'lan');
    this.value('wan', e.legs.wan, lim === 'wan');
    this.value('direct', e.legs.direct, lim === 'direct');
    if (e.legs.wan?.target) $('link-net').textContent = e.legs.wan.target;
  },
};

// Rough durations, for "about N s left".
const legSeconds = () => 4 + (settings.pretest ? 2 : 0) + (settings.download ? settings.downloadS : 0) + (settings.upload ? settings.uploadS : 0);
const LEG_EST = () => [legSeconds(), 24, legSeconds()];

function updateEta() {
  const el = $('eta');
  if (!state.three || !state.running || state.leg == null) { el.textContent = ''; return; }
  const est = LEG_EST();
  const inLeg = (performance.now() - state.legStart) / 1000;
  let left = Math.max(1, est[state.leg] - inLeg);
  for (let i = state.leg + 1; i < 3; i++) left += est[i];
  el.textContent = `${state.leg + 1} of 3 · about ${Math.round(left / 5) * 5 || 5} s left`;
}

function enterLeg(i) {
  state.leg = i;
  state.legStart = performance.now();
  state.phase = 'idle';
  markLeg(i);
  topo.active(LEGS[i].k);
  if (i > 0) ring.beginBand(i);
  updateEta();
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

/** Marks a browser measurement as running so the ring paces its frames. */
async function browserMeasures(p) {
  ring.measuring++;
  try {
    return await p;
  } finally {
    ring.measuring--;
  }
}

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
  three: false,      // this run measures the three legs
  leg: null,         // leg in progress (0..2)
  legStart: 0,
  etaTimer: 0,
  bg: null,          // background quality pass (promise)
  bgEntry: null,     // the entry it fills in
  bgKeys: [],        // strip items it will fill
  queued: false,     // "run again" pressed while the background pass runs
  upTarget: null,    // public server of the internet legs
  directUrl: null,   // URL this browser reached it on
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
  if (!state.ready || state.running || state.queued) return;
  if (state.bg) {
    // Wait for the background pass: it shares this thread and the line.
    state.queued = true;
    core.set({ phase: 'Finishing ping', waiting: true });
    try { await state.bg; } finally { state.queued = false; }
  }
  closeDrawers();
  const direct = internetOnly();
  const three = !direct && legsWanted() && legsValid();
  const defer = deferQuality();
  Object.assign(state, {
    running: true,
    result: null,
    view: null,
    partial: {},
    phase: 'idle',
    logLines: [],
    loss: lossMeasurable(),
    hiddenDuringRun: document.visibilityState !== 'visible',
    three,
    leg: null,
    bgEntry: null,
    bgKeys: [],
    upTarget: null,
    directUrl: null,
  });

  body.classList.remove('idle', 'done', 'legs', 'bg');
  body.classList.add('busy');
  body.classList.toggle('three', three);
  $('after').hidden = true;
  $('verdict').hidden = true;
  $('path').hidden = true;
  $('diag').hidden = true;
  message(remoteNote());
  strip.el.classList.remove('reveal');
  strip.build(plannedKeys());
  setupSteps(three);
  topo.setup(three, serverInfo?.path?.relation, direct ? 'nearest server' : null);
  ring.clear(three ? 3 : 1);
  if (three) {
    enterLeg(0);
    state.etaTimer = setInterval(updateEta, 1000);
  }
  enterPhase('connect');

  let urls = [serverUrl()];
  let via = null;
  if (direct) {
    const t = await internetTarget();
    if (t.urls.length) {
      urls = t.urls;
      via = t.target.name;
      topo.setup(false, 'self', via);
      message(`This device is the Nettest server, so it measures its own internet connection to ${via}.`);
    } else {
      message(`This device is the Nettest server, but no public server was reachable (${t.error}). Measuring the local server instead.`, 'error');
    }
  }
  const options = measurementOptions();
  if (defer) Object.assign(options, { ping: false, qos: false });
  if (via) Object.assign(options, { qos: false, save: false, signedResult: false });
  options.onProgress = onProgress;
  log(`options ${JSON.stringify(options)}`);

  let entry = null;
  try {
    let r = null;
    let url = urls[0];
    for (const [i, u] of urls.entries()) {
      url = u;
      log(`server ${url}`);
      let measured = false;
      const opts = { ...options, onProgress: (p) => { if (HALF[p.phase] != null && ok(p.mbps)) measured = true; onProgress(p); } };
      try {
        r = await browserMeasures(run_measurement(url, opts, log));
        if (!via || ok(r.downloadMbps) || ok(r.uploadMbps)) break;
      } catch (err) {
        // ws:// blocked by the network? The public server's wss:// port may work.
        if (measured || i === urls.length - 1) throw err;
        log(`error: ${err?.message || err}, trying the next port`);
      }
      if (measured) break;
      ring.resetLive();
    }
    state.prev = history().find((e) => e.server === url && !!e.legs === three) || null;
    state.result = r;
    entry = entryFrom(r, url);
    if (via) entry.via = via;
    if (three) await runLegs(entry);
    if (defer) {
      const keys = settings.ping ? ['ping', 'jitter'] : [];
      if (state.loss) keys.push('jitter', 'loss');
      state.bgKeys = [...new Set(keys)];
      state.bgEntry = entry;
    }
    finish(entry, r);
  } catch (e) {
    entry = null;
    fail(e);
  } finally {
    state.running = false;
    state.leg = null;
    clearInterval(state.etaTimer);
    updateEta();
    body.classList.remove('busy');
  }
  if (entry && state.bgKeys.length) {
    state.bg = runQuality(entry).finally(() => { state.bg = null; });
  }
}

/** Strip items: what was measured, plus what the background pass will add. */
function renderStrip(e, prev) {
  const pending = state.bgEntry === e ? state.bgKeys : [];
  const keys = METRICS.filter((m) => ok(e[m.k]) || pending.includes(m.k)).map((m) => m.k);
  strip.build(keys);
  for (const k of keys) {
    strip.set(k, e[k]);
    if (prev) strip.delta(k, e[k], prev[k]);
  }
}

/**
 * Ping, jitter and packet loss after the speeds: this device <-> server, the
 * server's line (quality run, same public server) and this device <-> the
 * public server. Each value appears as soon as it is known.
 */
async function runQuality(e) {
  const three = !!e.legs;
  body.classList.add('bg');
  $('eta').textContent = state.loss ? 'Measuring ping, jitter and packet loss…' : 'Measuring ping and jitter…';
  const quiet = { pretest: false, download: false, upload: false, ping: true, save: false, signedResult: false };

  const local = async () => {
    if (settings.ping || state.loss) {
      const onProgress = (p) => {
        if (!p.final) return;
        if (p.phase === 'ping') {
          e.ping = val(p.pingMs);
          e.jitter = val(p.jitterMs);
        } else if (p.phase === 'qos') {
          e.loss = val(p.packetLossPercent);
          if (ok(p.jitterMs)) e.jitter = p.jitterMs;
        } else return;
        refreshQuality(e);
      };
      const opts = { ...measurementOptions(), ...quiet, ping: settings.ping || state.loss, qos: state.loss, onProgress };
      try {
        const r = await withTimeout(run_measurement(e.server || serverUrl(), opts, (l) => log(`[quality] ${l}`)), 60_000, 'the quality measurement');
        e.ping = val(r.pingMs) ?? e.ping;
        e.jitter = val(r.jitterMs) ?? e.jitter;
        e.loss = val(r.packetLossPercent);
        e.transport = r.qosTransport;
      } catch (err) {
        log(`[quality] error: ${err?.message || err}`);
      }
      refreshQuality(e);
    }
    // This device <-> the public server: ping only, same URL as its speeds.
    if (three && settings.ping && state.directUrl && e.legs.direct && !e.legs.direct.error) {
      try {
        const r = await withTimeout(
          run_measurement(state.directUrl, { ...measurementOptions(), ...quiet, qos: false }, (l) => log(`[internet] ${l}`)),
          30_000, 'the internet ping');
        e.legs.direct.ping = val(r.pingMs);
        e.legs.direct.jitter = val(r.jitterMs);
      } catch (err) {
        log(`[internet] ping error: ${err?.message || err}`);
      }
      refreshQuality(e);
    }
  };

  const server = async () => {
    if (!three || !settings.ping || !e.legs.wan || e.legs.wan.error) return;
    const out = await runUpstream('quality', false);
    const r = out?.result;
    if (r) Object.assign(e.legs.wan, { ping: val(r.pingMs), jitter: val(r.jitterMs), loss: val(r.packetLossPercent) });
    refreshQuality(e);
  };

  await Promise.all([local(), server()]);
  state.bgKeys = [];
  state.bgEntry = null;
  body.classList.remove('bg');
  $('eta').textContent = '';
  refreshQuality(e);
  replaceHistory(e);
  if (state.view === e) ring.canvas.setAttribute('aria-label', summaryLine(e));
}

/** Put background values on screen, if this run is still the one shown. */
function refreshQuality(e) {
  if (state.view !== e) return;
  renderStrip(e, state.prev);
  renderVerdict(e, false);
  if (e.legs) {
    legsView.patch('lan', { ping: e.ping });
    legsView.patch('wan', { ping: e.legs.wan?.ping });
    legsView.patch('direct', { ping: e.legs.direct?.ping });
    topo.show(e);
  }
}

/** Ring slots for storage: 3 significant digits, null for empty. */
const compact = (arr) => (arr ? arr.map((v) => (ok(v) ? Number(v.toPrecision(3)) : null)) : null);
const val = (v) => (ok(v) ? v : null);

function entryFrom(r, url) {
  const raw = ring.bandRaw(0);
  return {
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
    ring: { d: compact(raw[0]), u: compact(raw[1]) },
  };
}

function finish(entry, r) {
  markStep('done');
  markLeg(3);
  topo.active(null);
  ring.finish();
  showView(entry, state.prev);
  addHistory(entry);

  const notes = [];
  if (entry.via) notes.push(`This device is the Nettest server, so this is its own internet connection, measured to ${entry.via}.`);
  if (r.downloadError) notes.push(`Download: ${r.downloadError}.`);
  if (r.uploadError) notes.push(`Upload: ${r.uploadError}.`);
  if (state.hiddenDuringRun) notes.push('The tab was in the background during the test, which can lower the results.');
  if (r.openTestUuid) notes.push(`Saved as ${r.openTestUuid}.`);
  message(notes.join(' '), r.downloadError || r.uploadError ? 'error' : '');

  const pv = entry.legs && pathVerdict(entry);
  $('announce').textContent = `Measurement complete. ${pv ? `${pv.title}. ` : ''}${summaryLine(entry)}`;
  if (fx()) navigator.vibrate?.(14);
}

/** Put a finished run on screen: centre number, strip, verdict, actions. */
function showView(e, prev) {
  state.view = e;
  setMode('done');
  body.classList.remove('idle');
  body.classList.add('done');
  const three = !!e.legs;
  body.classList.toggle('legs', three);
  body.classList.toggle('three', three);
  topo.show(e);

  // With the legs, the centre shows what actually reaches this device.
  const eff = three ? effective(e) : e;
  const head = ok(eff.download) ? 'download' : ok(eff.upload) ? 'upload' : null;
  const reach = three && eff !== e && head;
  core.set({
    phase: reach ? 'End to end' : head ? PHASE_LABEL[head] : ok(e.ping) ? PHASE_LABEL.ping : PHASE_LABEL.done,
    value: head ? eff[head] : ok(e.ping) ? e.ping : '✓',
    unit: head ? 'Mbit/s' : ok(e.ping) ? 'ms' : '',
    cta: noHover ? 'Tap to run again' : 'Run again · Space',
    button: 'Run the measurement again',
  });

  // Only what was measured (plus what is still coming): the grid keeps any count even.
  renderStrip(e, prev);
  restartAnimation(strip.el, 'reveal');

  renderVerdict(e);
  renderDiag(e);
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

function renderVerdict(e, animate = true) {
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
  if (s && animate) restartAnimation(el, 'reveal');
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

/** Store values that arrived after the run was saved. */
function replaceHistory(entry) {
  const h = history();
  const i = h.findIndex((x) => x.ts === entry.ts);
  if (i < 0) return;
  h[i] = entry;
  writeJson(STORE_HISTORY, h);
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
    paintBands(ctx, 26, 26, 21, entryBands(e), { colors, track: false, glow: false });

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
  paintBands(ctx, cx, cy, 225, entryBands(e), { colors: c });

  // With the legs the centre is what reaches this device end to end.
  const eff = e.legs ? effective(e) : e;
  const head = ok(eff.download) ? 'download' : ok(eff.upload) ? 'upload' : 'ping';
  const m = METRICS.find((x) => x.k === head);
  ctx.textAlign = 'center';
  ctx.fillStyle = c.muted;
  ctx.font = `500 14px ${mono}`;
  ctx.fillText((e.legs && eff !== e ? 'End to end' : m.label).toUpperCase().split('').join(' '), cx, cy - 58);
  ctx.fillStyle = c.ink;
  ctx.font = `200 ${e.legs ? 80 : 92}px ${sans}`;
  ctx.fillText(m.fmt(eff[head]), cx, cy + 30);
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
  const wrap = (text, y, lh) => {
    let line = '';
    for (const word of text.split(' ')) {
      const next = line ? `${line} ${word}` : word;
      if (ctx.measureText(next).width > 500 && line) {
        ctx.fillText(line, x0, y);
        line = word;
        y += lh;
      } else line = next;
    }
    if (line) ctx.fillText(line, x0, y);
    return y + lh;
  };

  const pv = e.legs && pathVerdict(e);
  if (pv) {
    // Diagnosis first, then one row per leg.
    const accent = pv.kind === 'warn' ? c.u2 : pv.kind === 'good' ? c.d1 : c.ink;
    ctx.fillStyle = c.muted;
    ctx.font = `500 13px ${mono}`;
    ctx.fillText((pv.eyebrow || '').toUpperCase().split('').join(' '), x0, 170);
    ctx.fillStyle = accent;
    ctx.font = `600 38px ${sans}`;
    let y = wrap(pv.big || pv.title, 212, 44);
    if (pv.meter) {
      const m = pv.meter, w = 470, h = 10;
      const f = m.cap > 0 ? Math.max(0.02, Math.min(1, m.got / m.cap)) : 1;
      ctx.fillStyle = c.line;
      ctx.beginPath(); ctx.roundRect(x0, y - 4, w, h, h / 2); ctx.fill();
      ctx.fillStyle = accent;
      ctx.beginPath(); ctx.roundRect(x0, y - 4, w * f, h, h / 2); ctx.fill();
      y += 34;
      ctx.fillStyle = c.ink;
      ctx.font = `500 22px ${sans}`;
      ctx.fillText(`${fmtNum(m.got)} of ${fmtNum(m.cap)} Mbit/s`, x0, y);
      ctx.fillStyle = c.muted;
      ctx.font = `16px ${sans}`;
      y = wrap(`${cap(m.gotLabel)} · ${m.capLabel} ${fmtNum(m.cap)}`, y + 26, 22);
    }
    y += 16;
    const rows = [['lan', { download: e.download, upload: e.upload, ping: e.ping }], ['wan', e.legs.wan], ['direct', e.legs.direct]];
    for (const [k, v] of rows) {
      const leg = LEGS.find((l) => l.k === k);
      ctx.fillStyle = pv.limit?.leg === k ? c.u2 : c.muted;
      ctx.font = `500 13px ${mono}`;
      ctx.fillText(leg.short.toUpperCase(), x0, y);
      ctx.fillStyle = c.ink;
      ctx.font = `20px ${sans}`;
      ctx.fillText(v && !v.error ? legSummary(v) : 'not measured', x0 + 210, y);
      y += 34;
    }
  } else {
    const big = METRICS.filter((mm) => mm.big && ok(e[mm.k]));
    const small = METRICS.filter((mm) => !mm.big && ok(e[mm.k]));
    big.forEach((mm, i) => metric(mm, x0 + i * 270, 200, 58));
    small.forEach((mm, i) => metric(mm, x0 + i * 180, 330, 38));
    ctx.fillStyle = c.muted;
    ctx.font = `17px ${sans}`;
    wrap(verdictText(e).s, 452, 26);
  }

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
/** `nettest -s -demo`: every segment runs, whatever the path. */
const demo = () => !!serverInfo?.demo;
/** Server's pre-run guess: does this device share its internet line? Older servers: assume so. */
const legsValid = () => demo() || (serverInfo?.path?.legsValid ?? true);
const relation = () => serverInfo?.path?.relation ?? null;
/**
 * The browser runs on the server itself: device <-> server is loopback and
 * server <-> internet is this device's own line, so only one measurement
 * means anything: this page against the nearest public server.
 */
const internetOnly = () => legsWanted() && relation() === 'self' && !demo();
const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1);

/** Why a capable server only runs the plain test for this visitor. */
function remoteNote() {
  if (!legsWanted() || legsValid()) return '';
  const lan = serverInfo?.path?.lanUrl;
  if (relation() === 'hairpin') {
    return `You opened the server by its public address, so only this device ↔ server is measured.${lan ? ` Open ${lan} for the full breakdown.` : ''}`;
  }
  return 'You are reaching this server over the internet, so only this device ↔ server is measured. The breakdown works from the server’s own network.';
}

const LEGS = [
  { k: 'lan', icon: 'i-device', label: 'This device ↔ Server', short: 'Device ↔ Server' },
  { k: 'wan', icon: 'i-server', label: 'Server ↔ Internet', short: 'Server ↔ Internet' },
  { k: 'direct', icon: 'i-globe', label: 'This device ↔ Internet', short: 'Device ↔ Internet' },
];

function legSummary(v) {
  const parts = [];
  if (ok(v.download)) parts.push(`↓ ${fmtNum(v.download)}`);
  if (ok(v.upload)) parts.push(`↑ ${fmtNum(v.upload)}`);
  let s = parts.length ? `${parts.join(' ')} Mbit/s` : '';
  if (ok(v.ping)) s += `${s ? ' · ' : ''}${fmtNum(v.ping)} ms`;
  return s;
}

/** Comparison table: one row per leg, one scale for every bar. */
const legsView = {
  rows: {},
  values: {},

  build() {
    const box = $('path');
    box.textContent = '';
    this.rows = {};
    this.values = {};
    const head = document.createElement('div');
    head.className = 'lh';
    head.setAttribute('aria-hidden', 'true');
    for (const t of ['Segment', '↓ Download', '↑ Upload', 'Ping']) {
      const s = document.createElement('span');
      s.textContent = t;
      head.append(s);
    }
    box.append(head);
    for (const leg of LEGS) {
      const el = document.createElement('div');
      el.className = 'leg waiting';
      el.dataset.leg = leg.k;
      const name = document.createElement('span');
      name.className = 'leg-name';
      name.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true"><use href="#${leg.icon}"/></svg>`;
      const label = document.createElement('span');
      label.textContent = leg.label;
      const sub = document.createElement('small');
      name.append(label, sub);
      el.append(name);
      const cells = {};
      for (const [cls, unit, title] of [['dn', 'Mbit/s', 'Download'], ['up', 'Mbit/s', 'Upload'], ['pg', 'ms', 'Ping']]) {
        const c = document.createElement('span');
        c.className = `c ${cls}`;
        c.title = title;
        const b = document.createElement('b');
        const i = document.createElement('i');
        c.append(b, i);
        el.append(c);
        cells[cls] = { c, b, i, unit };
      }
      box.append(el);
      this.rows[leg.k] = { el, sub, cells };
    }
    box.hidden = false;
  },

  /** `v`: {download, upload, ping, state, text, error, limitDir}. */
  set(k, v) {
    const row = this.rows[k];
    if (!row) return;
    this.values[k] = v;
    const st = v.error ? 'failed' : v.state || 'done';
    row.el.className = `leg ${st}${v.limitDir ? ' limit' : ''}`;
    row.sub.textContent = v.error || v.text || '';
    const put = (cell, x, limit) => {
      cell.c.classList.toggle('limit', !!limit);
      cell.b.textContent = ok(x) ? fmtNum(x) : '–';
      if (ok(x)) {
        const s = document.createElement('small');
        s.textContent = cell.unit;
        cell.b.append(s);
      }
    };
    put(row.cells.dn, v.download, v.limitDir === 'download');
    put(row.cells.up, v.upload, v.limitDir === 'upload');
    put(row.cells.pg, v.ping, false);
    this.scale();
  },

  /** Merge late values (ping after the speeds) into a finished row. */
  patch(k, v) {
    const cur = this.values[k];
    if (!cur || cur.error) return;
    this.set(k, { ...cur, ...v });
  },

  scale() {
    let max = 0;
    for (const v of Object.values(this.values)) {
      if (ok(v.download)) max = Math.max(max, v.download);
      if (ok(v.upload)) max = Math.max(max, v.upload);
    }
    for (const [k, row] of Object.entries(this.rows)) {
      const v = this.values[k] || {};
      const w = (x) => (ok(x) && max > 0 ? Math.max(0.015, x / max).toFixed(4) : '0');
      row.cells.dn.i.style.setProperty('--w', w(v.download));
      row.cells.up.i.style.setProperty('--w', w(v.upload));
    }
  },

  /** A finished run, live or from history: only if its legs were measured. */
  show(e) {
    if (!e.legs) {
      $('path').hidden = true;
      return;
    }
    this.build();
    const lim = pathVerdict(e)?.limit;
    const limitDir = (k) => (lim?.leg === k ? lim.dir : null);
    this.set('lan', { download: e.download, upload: e.upload, ping: e.ping, limitDir: limitDir('lan') });
    for (const k of ['wan', 'direct']) {
      const v = e.legs[k];
      this.set(k, v
        ? { ...v, text: v.error ? '' : v.target ? `via ${v.target}` : '', limitDir: limitDir(k) }
        : { error: 'not measured' });
    }
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
  select: 'Finding a server', idle: 'Starting', greeting: 'Connecting', init: 'Preparing',
  ping: 'Ping', jitter: 'Jitter', packetloss: 'Packet loss', download: 'Download', upload: 'Upload',
};
const SERVER_LEG_MS = 7000; // the server's client measures 7 s per direction

/** Put the server's live phase on the ring and in the centre. */
const upstreamView = {
  phase: null,
  since: 0,

  reset() { this.phase = null; },

  update(r) {
    const half = HALF[r.phase];
    if (r.phase !== this.phase) {
      if (HALF[this.phase] != null) ring.complete(HALF[this.phase]);
      this.phase = r.phase;
      this.since = performance.now();
      const label = UPSTREAM_PHASE[r.phase] ?? cap(String(r.phase || 'Working'));
      if (half != null) {
        setMode(r.phase);
        ring.setRadar(false);
        ring.tip = { half, t: 0 };
        core.set({ phase: label, value: 0, unit: 'Mbit/s' });
      } else if (r.phase === 'ping') {
        setMode('ping');
        ring.setRadar(true);
        core.set({ phase: label, value: '', unit: 'ms', waiting: true });
      } else {
        setMode('busy');
        ring.setRadar(true);
        core.set({ phase: label, waiting: true });
      }
    }
    if (half != null) {
      const elapsed = ok(r.elapsedMs) ? r.elapsedMs : performance.now() - this.since;
      const t = Math.min(1, elapsed / SERVER_LEG_MS);
      core.phase.textContent = `${PHASE_LABEL[r.phase]} · ${(elapsed / 1000).toFixed(1)} s`;
      if (ok(r.mbps) && elapsed > 250) {
        ring.sample(half, t, r.mbps);
        core.number(r.mbps);
      } else {
        ring.tip = { half, t };
        ring.kick();
      }
    } else if (r.phase === 'ping' && ok(r.pingMs) && !ok(core.target)) {
      core.set({ phase: 'Ping', value: r.pingMs, unit: 'ms' });
    }
  },

  /** Final values: halves without live samples (upload) get a flat ring. */
  settle(result) {
    if (!result) return;
    [result.downloadMbps, result.uploadMbps].forEach((v, h) => {
      if (ring.raw[h]) ring.complete(h);
      else if (ok(v)) ring.fill(h, v);
    });
    ring.tip = null;
    ring.kick();
  },
};

/**
 * Ask the server to measure its own internet connection and follow it.
 * `kind`: speed (download/upload) or quality (ping, jitter, loss; quiet).
 */
async function runUpstream(kind = 'speed', show = true) {
  // The browser only polls during the server's leg, so the ring can animate freely.
  if (show) ring.idle = true;
  try {
    return await pollUpstream(kind, show);
  } finally {
    ring.idle = false;
  }
}

async function pollUpstream(kind, show) {
  if (show) {
    legsView.set('wan', { state: 'running', text: 'starting' });
    upstreamView.reset();
  }
  let start;
  try {
    start = await api(`api/upstream/start?kind=${kind}`, 'POST');
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
    await sleep(show ? 150 : 400);
    let s;
    try { s = (await api('api/upstream/status')).data; } catch { continue; }
    if (s.running && s.run) {
      id ??= s.run.id;
      const r = s.run;
      if (!show) continue;
      upstreamView.update(r);
      if (r.target?.name) $('link-net').textContent = r.target.name;
      const live = { download: r.downloadMbps, upload: r.uploadMbps };
      if (HALF[r.phase] != null && ok(r.mbps)) live[r.phase] = r.mbps;
      legsView.set('wan', {
        state: 'running', download: live.download, upload: live.upload, ping: r.pingMs,
        text: `${(UPSTREAM_PHASE[r.phase] ?? r.phase ?? '').toLowerCase()}${r.target ? ` · ${r.target.name}` : ''}`,
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

/**
 * Plain-text URLs for the public server. ws:// uses the IPv4 address the
 * server measured (same address family on both legs); wss:// needs the name
 * for its certificate.
 */
function targetUrls(target, scheme) {
  const out = [];
  if (target.wsPort && location.protocol !== 'https:') out.push(`${scheme}://${hostPart(target.ip || target.host)}:${target.wsPort}`);
  if (target.wssPort) out.push(`${scheme}s://${hostPart(target.host)}:${target.wssPort}`);
  return out;
}

/** The public server the Nettest server would use, without measuring anything. */
async function internetTarget() {
  const out = await pollUpstream('target', false);
  const target = out?.target;
  if (!target || out.error) return { urls: [], error: out?.error || 'no public server' };
  const urls = targetUrls(target, 'ws').map((u) => `${u}/`);
  return { target, urls, error: urls.length ? '' : 'the public server has no browser port' };
}

/** This page against the same public server the server used. */
async function runDirect(target) {
  const urls = targetUrls(target, 'ws').map((u) => `${u}/`);
  if (!urls.length) return { error: 'the public server has no browser port' };
  let lastError = '';
  for (const url of urls) {
    const live = {};
    let measured = false;
    state.phase = 'idle';
    const progress = (p) => {
      onProgress(p);
      if (HALF[p.phase] != null && ok(p.mbps)) { live[p.phase] = p.mbps; measured = true; }
      if (p.phase === 'ping' && p.final) { live.ping = p.pingMs; measured = true; }
      legsView.set('direct', {
        state: 'running', download: live.download, upload: live.upload, ping: live.ping,
        text: PHASE_LABEL[p.phase]?.toLowerCase() ?? p.phase,
      });
    };
    const options = {
      ...measurementOptions(), qos: false, save: false, signedResult: false, onProgress: progress,
      ping: settings.ping && !deferQuality(),
    };
    log(`[internet] server ${url}`);
    try {
      const r = await withTimeout(browserMeasures(run_measurement(url, options, (l) => log(`[internet] ${l}`))), 120_000, 'the direct measurement');
      if (ok(r.downloadMbps) || ok(r.uploadMbps)) {
        state.directUrl = url;
        return { download: val(r.downloadMbps), upload: val(r.uploadMbps), ping: val(r.pingMs) };
      }
      lastError = r.downloadError || r.uploadError || 'no data transferred';
    } catch (e) {
      lastError = e?.message || String(e);
      log(`[internet] error: ${lastError}`);
    }
    if (measured) break; // it connected and then failed: another port will not help
    ring.resetLive();
    ring.kick();
  }
  return { error: /connect|closed|refused|failed/i.test(lastError) ? 'could not reach the public server from this browser' : lastError };
}

/** This browser's public address as the public server sees it (newer servers only). */
async function publicIpVia(target) {
  for (const base of targetUrls(target, 'http')) {
    try {
      const res = await withTimeout(fetch(`${base}/api/ip`, { cache: 'no-store' }), 3000, 'ip echo');
      if (res.ok) {
        const ip = (await res.json())?.ip;
        if (ip) return ip;
      }
    } catch { /* older server or blocked: unknown */ }
  }
  return null;
}

function expand6(ip) {
  const [h, t] = ip.split('::');
  const a = h ? h.split(':') : [];
  const b = t ? t.split(':') : [];
  const mid = t === undefined ? [] : new Array(Math.max(0, 8 - a.length - b.length)).fill('0');
  return [...a, ...mid, ...b].map((x) => parseInt(x || '0', 16));
}

/**
 * Do two public addresses leave through one line? Same IPv4 (one NAT) or the
 * same IPv6 /56. null when unknown (missing, or different address families).
 */
function sameExit(a, b) {
  if (!a || !b) return null;
  const v6 = (x) => x.includes(':');
  if (v6(a) !== v6(b)) return null;
  if (!v6(a)) return a === b;
  const x = expand6(a), y = expand6(b);
  return x[0] === y[0] && x[1] === y[1] && x[2] === y[2] && (x[3] >> 8) === (y[3] >> 8);
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
    // Line load is only known where the server can read it (Linux).
    busy: r.otherTrafficBytes == null ? null : !!r.busy,
    cpuLimited: !!r.cpuLimited, cpuPeak: val(r.cpuPeakPercent),
    publicIp: r.publicIp || null,
    ageS: outcome.reused ? outcome.ageS : undefined,
  };
}

const ringOf = (i) => {
  const raw = ring.bandRaw(i);
  return { d: compact(raw[0]), u: compact(raw[1]) };
};

async function runLegs(e) {
  e.legs = { relation: relation(), cgnat: !!serverInfo?.path?.cgnat, demo: demo() };
  body.classList.add('legs'); // the table takes over from the strip
  legsView.build();
  legsView.set('lan', { download: e.download, upload: e.upload, ping: e.ping });
  topo.value('lan', { download: e.download, upload: e.upload });

  // 2: server <-> internet.
  enterLeg(1);
  const up = await runUpstream(deferQuality() ? 'speed' : 'full');
  upstreamView.settle(up.result);
  e.legs.wan = legFrom(up);
  if (!e.legs.wan.error) e.legs.wan.ring = ringOf(1);
  legsView.set('wan', { ...e.legs.wan, text: e.legs.wan.target && !e.legs.wan.error ? `via ${e.legs.wan.target}` : '' });
  topo.value('wan', e.legs.wan);
  if (up.target?.name) $('link-net').textContent = up.target.name;

  // 3: this device <-> the same public server. Pointless when this device is
  // the server: it would measure the same path again and only add noise.
  if (e.legs.relation === 'self' && !demo()) {
    e.legs.direct = { skipped: true };
    legsView.set('direct', { state: 'done', text: 'same as Server ↔ Internet: this device is the server' });
    markLeg(LEGS.length);
  } else if (up.target && (up.target.wsPort || up.target.wssPort)) {
    enterLeg(2);
    legsView.set('direct', { state: 'running', text: `connecting to ${up.target.name}` });
    e.legs.direct = { ...(await runDirect(up.target)), target: up.target.name };
    if (!e.legs.direct.error) {
      e.legs.direct.ring = ringOf(2);
      e.legs.direct.publicIp = await publicIpVia(up.target);
    }
  } else {
    e.legs.direct = { error: 'skipped: no public server' };
  }
  e.legs.sameExit = sameExit(e.legs.wan.publicIp, e.legs.direct.publicIp);
  log(`[path] relation ${e.legs.relation ?? 'unknown'}, public ${e.legs.wan.publicIp ?? '?'} / ${e.legs.direct.publicIp ?? '?'}, same exit ${e.legs.sameExit}`);
  state.phase = 'done';
}

/**
 * Where the bottleneck is: {kind, title, parts: [[text, bold]], tip, chips,
 * limit: {leg, dir}, text}. Download first, upload if download is missing.
 */
function pathVerdict(e) {
  const L = e.legs;
  if (!L) return null;
  const wan = L.wan;
  const direct = L.direct && !L.direct.error ? L.direct : null;
  const T = (t) => [t, false];
  const B = (t) => [t, true];
  const chips = [];
  const done = (v) => ({
    chips, tip: '', limit: null, meter: null, ...v,
    text: `${v.title}. ${v.meter ? `${fmtNum(v.meter.got)} of ${fmtNum(v.meter.cap)} Mbit/s ${v.meter.dir}. ` : ''}${v.parts.map((p) => p[0]).join('')}`,
  });

  if (!wan || wan.error) {
    if (direct && ok(direct.download)) {
      return done({
        kind: 'info',
        title: 'Only part of the path was measured',
        parts: [T(`The server could not measure its own connection (${wan?.error || 'not measured'}). This device reaches the internet at `), B(`${fmtNum(direct.download)} Mbit/s`), T(' download.')],
      });
    }
    return done({ kind: 'info', title: 'The internet side could not be measured', parts: [T(`${cap(wan?.error || 'the server did not report a result')}.`)] });
  }
  if (L.sameExit === false) {
    chips.push('different public addresses');
    return done({
      kind: 'info',
      title: 'Separate paths',
      parts: [T('This device and the server reach the internet through different connections, so the three results do not combine into one diagnosis.')],
    });
  }

  const gw = L.relation === 'gateway';
  // In the browser on the server itself the local leg is just loopback.
  const lan = L.relation === 'self' && !L.demo ? {} : { download: e.download, upload: e.upload, ping: e.ping };
  const dir = ok(lan.download) && ok(wan.download) ? 'download' : ok(lan.upload) && ok(wan.upload) ? 'upload' : null;
  const d = dir ? direct?.[dir] : null;
  let b = dir ? wan[dir] : null;
  // A saturated server CPU understates the line: the direct figure is better.
  const cpuLow = !!(dir && wan.cpuLimited && ok(d) && d > b * 1.15);
  if (cpuLow) b = d;
  const M = (got, capv, gotLabel, capLabel) => ({ got, cap: capv, dir, gotLabel, capLabel });
  let v;

  if (dir) {
    const a = lan[dir];
    const expected = Math.min(a, b);
    // Two back-to-back internet measurements easily differ by 30-50 %
    // (shared public server, cross traffic). Blame the path only when it is
    // clearly slower in both directions, not on one noisy figure.
    const other = dir === 'download' ? 'upload' : 'download';
    const otherExp = Math.min(lan[other], wan[other]);
    const otherLow = !ok(direct?.[other]) || !ok(otherExp) || direct[other] < otherExp * 0.8;
    if (ok(d) && d < expected * 0.6 && otherLow) {
      v = {
        kind: 'warn',
        limit: { leg: 'direct', dir },
        eyebrow: 'Slowed down by',
        big: gw ? 'The server' : 'Something in between',
        title: gw ? 'The server is slowing traffic down' : 'Something on the way out slows this device down',
        parts: [T('Your Wi-Fi and your line are both faster than what arrives here.')],
        meter: M(d, expected, 'reaches this device', 'each part can do'),
        tip: gw ? 'Check traffic shaping (QoS) and the CPU load on the server.' : 'A VPN, traffic shaping or a busy gateway is the usual cause.',
      };
    } else if (a < b * 0.8) {
      v = {
        kind: 'warn',
        limit: { leg: 'lan', dir },
        eyebrow: 'Slowed down by',
        big: gw ? 'Your Wi-Fi' : 'Your local network',
        title: gw ? 'Your Wi-Fi or local network is the bottleneck' : 'The local connection is the bottleneck',
        parts: [T('Your internet is faster than what reaches this device.')],
        meter: M(a, b, 'reaches this device', 'your internet delivers'),
        tip: 'Move closer to the access point, use 5 or 6 GHz, or try a cable.',
      };
    } else if (b < a * 0.8) {
      v = {
        kind: 'warn',
        limit: { leg: 'wan', dir },
        eyebrow: 'Slowed down by',
        big: 'Your internet line',
        title: 'Your internet line is the bottleneck',
        parts: [T('Your network is fine. This is all your provider delivers.')],
        meter: M(b, a, 'your internet delivers', 'your network can carry'),
        tip: 'If that is less than you pay for, contact your provider.',
      };
    } else {
      v = {
        kind: 'good',
        eyebrow: 'No bottleneck',
        big: 'Full speed',
        title: 'Local network and internet line are well matched',
        parts: [T('Your network and your internet line keep up with each other.')],
        meter: M(ok(d) ? Math.min(d, expected) : expected, expected, 'reaches this device', 'your internet delivers'),
      };
    }
  } else {
    const x = ok(wan.download) ? ['download', wan.download] : ['upload', wan.upload];
    v = {
      kind: 'info', eyebrow: 'Your internet', big: `${fmtNum(x[1])} Mbit/s`,
      title: 'Your internet line', parts: [T(`What the server measured on its own connection (${x[0]}).`)],
    };
  }

  // Only notes that change how to read the result; the rest is in Details.
  if (dir && ok(d) && ok(wan[dir]) && v.limit?.leg !== 'direct' && Math.abs(d - wan[dir]) > 0.3 * Math.max(d, wan[dir])) {
    chips.push('internet speed varied between the two runs');
  }
  if (cpuLow) chips.push('server CPU was maxed out, so the direct figure is used');
  if (wan.busy === true) chips.push('other devices used the internet during the test');
  if (ok(wan.ageS) && wan.ageS > 0) chips.push(`server result is ${Math.max(1, Math.round(wan.ageS / 60))} min old`);
  return done(v);
}

function renderDiag(e) {
  const box = $('diag');
  const pv = e.legs && pathVerdict(e);
  if (!pv) {
    box.hidden = true;
    return;
  }
  box.className = `diag ${pv.kind}`;
  $('diag-eyebrow').textContent = pv.eyebrow || '';
  $('diag-title').textContent = pv.big || pv.title;
  const m = pv.meter;
  const meter = $('diag-meter');
  meter.hidden = !m;
  if (m) {
    const pct = m.cap > 0 ? Math.max(2, Math.min(100, (m.got / m.cap) * 100)) : 100;
    meter.style.setProperty('--fill', `${pct}%`);
    $('diag-got').textContent = fmtNum(m.got);
    $('diag-got-label').textContent = `Mbit/s ${m.gotLabel}`;
    $('diag-cap').textContent = fmtNum(m.cap);
    $('diag-cap-label').textContent = m.capLabel;
    meter.classList.toggle('full', pct >= 92);
    meter.setAttribute('aria-label', `${fmtNum(m.got)} of ${fmtNum(m.cap)} Mbit/s ${m.dir}`);
  }
  $('diag-text').textContent = pv.parts.map((p) => p[0]).join('');
  $('diag-tip').textContent = pv.tip;
  const ul = $('diag-chips');
  ul.textContent = '';
  for (const c of pv.chips) {
    const li = document.createElement('li');
    li.textContent = c;
    ul.append(li);
  }
  box.hidden = false;
  restartAnimation(box, 'reveal');
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
  const L = state.view?.legs;
  if (L) {
    rows.push(['Path', `${L.relation ?? 'unknown'}${L.cgnat ? ' · CGNAT' : ''}`]);
    for (const leg of LEGS.slice(1)) {
      const v = L[leg.k];
      if (v) rows.push([leg.short, v.error ? `not measured: ${v.error}` : `${legSummary(v)}${v.target ? ` · ${v.target}` : ''}`]);
    }
    if (L.wan?.cpuPeak != null) rows.push(['Server CPU peak', `${Math.round(L.wan.cpuPeak)} %`]);
    rows.push(['Public address', `server ${L.wan?.publicIp ?? '?'} · this device ${L.direct?.publicIp ?? '?'} (${L.sameExit == null ? 'unknown' : L.sameExit ? 'same exit' : 'different exits'})`]);
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
    if (v && !v.error) lines.push(`${`${leg.short}:`.padEnd(19)}${legSummary(v)}${v.target ? ` (${v.target})` : ''}`);
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
  topo.serverHost = host || '–';
  $('link').title = `Measuring the path between this device and ${host}`;
  try {
    const res = await fetch('api/info', { cache: 'no-store' });
    if (!res.ok) throw new Error(res.statusText);
    const info = await res.json();
    serverInfo = info;
    $('link-client').textContent = info.clientIp || 'this browser';
    $('link-server-name').textContent = settings.server.trim() ? 'Server' : info.name || 'Nettest server';
    topo.serverName = $('link-server-name').textContent;
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
