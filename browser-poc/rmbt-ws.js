// TEMPORARY browser PoC — RMBT-over-WebSocket (RMBTws) client.
//
// Runs the RMBT greeting → ping → download phases over a browser WebSocket
// (works in Node 22+ too, which has a global WebSocket). Jitter/packet-loss are
// UDP-only and intentionally skipped here.
//
// The measurement server accepts a standard browser WebSocket handshake
// (worker.rs detects `upgrade: websocket` and finishes a normal WS handshake),
// then speaks the RMBT byte protocol over WS frames. This client mirrors that.

const NL = 0x0a;
const TERMINATOR = 0xff; // last byte of the last download chunk

/// Pull-based reader over the event-driven WebSocket. Two modes:
///  - line mode: buffer bytes, hand out `\n`-terminated lines
///  - download mode: don't buffer, just count bytes and watch chunk boundaries
class RmbtIo {
  constructor(ws) {
    this.ws = ws;
    ws.binaryType = 'arraybuffer';
    this.buf = new Uint8Array(0);
    this.lineWaiter = null; // { resolve, reject, timer }
    this.mode = 'line';
    this.closed = false;

    ws.onmessage = (e) => {
      let bytes;
      if (typeof e.data === 'string') bytes = new TextEncoder().encode(e.data);
      else bytes = new Uint8Array(e.data);
      if (this.mode === 'download') this._downloadFeed(bytes);
      else this._append(bytes);
    };
    ws.onclose = () => { this.closed = true; this._failLine(new Error('socket closed')); };
    ws.onerror = () => { this.closed = true; this._failLine(new Error('socket error')); };
  }

  open() {
    return new Promise((resolve, reject) => {
      if (this.ws.readyState === 1) return resolve();
      this.ws.onopen = () => resolve();
      this.ws.onerror = () => reject(new Error('connect failed'));
    });
  }

  send(str) {
    this.ws.send(new TextEncoder().encode(str));
  }

  _append(bytes) {
    const merged = new Uint8Array(this.buf.length + bytes.length);
    merged.set(this.buf);
    merged.set(bytes, this.buf.length);
    this.buf = merged;
    this._tryLine();
  }

  _tryLine() {
    if (!this.lineWaiter) return;
    const nl = this.buf.indexOf(NL);
    if (nl >= 0) {
      const line = new TextDecoder().decode(this.buf.subarray(0, nl)).replace(/\r$/, '');
      this.buf = this.buf.subarray(nl + 1);
      const w = this.lineWaiter;
      this.lineWaiter = null;
      clearTimeout(w.timer);
      w.resolve(line);
    }
  }

  _failLine(err) {
    if (this.lineWaiter) {
      const w = this.lineWaiter;
      this.lineWaiter = null;
      clearTimeout(w.timer);
      w.reject(err);
    }
  }

  readLine(timeoutMs = 8000) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.lineWaiter = null;
        reject(new Error('readLine timeout'));
      }, timeoutMs);
      this.lineWaiter = { resolve, reject, timer };
      this._tryLine();
    });
  }

  // Read lines, discarding any that don't start with `prefix`, return the match.
  async readUntil(prefix, timeoutMs = 8000) {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const line = await this.readLine(Math.max(1, deadline - Date.now()));
      if (line.startsWith(prefix)) return line;
    }
  }

  // Switch to download mode: count bytes until a chunk ends with 0xFF.
  download(chunkSize, onProgress) {
    return new Promise((resolve) => {
      this.dl = { bytes: 0, chunkPos: 0, chunkSize, onProgress, start: perfNow(), resolve };
      this.mode = 'download';
      // Feed any bytes already buffered from line mode.
      if (this.buf.length) {
        const leftover = this.buf;
        this.buf = new Uint8Array(0);
        this._downloadFeed(leftover);
      }
    });
  }

  _downloadFeed(bytes) {
    const dl = this.dl;
    let i = 0;
    while (i < bytes.length) {
      const remaining = dl.chunkSize - dl.chunkPos;
      const take = Math.min(remaining, bytes.length - i);
      dl.chunkPos += take;
      dl.bytes += take;
      i += take;
      if (dl.chunkPos === dl.chunkSize) {
        const flag = bytes[i - 1]; // last byte of this chunk
        dl.chunkPos = 0;
        if (flag === TERMINATOR) {
          this.mode = 'line';
          const elapsed = (perfNow() - dl.start) / 1000;
          const total = dl.bytes;
          if (i < bytes.length) this._append(bytes.subarray(i)); // trailing line-mode bytes
          this.dl = null;
          dl.resolve({ bytes: total, elapsed });
          return;
        }
      }
    }
    if (dl.onProgress) dl.onProgress(dl.bytes, (perfNow() - dl.start) / 1000);
  }
}

function perfNow() {
  return (typeof performance !== 'undefined' ? performance.now() : Date.now());
}

function uuid4() {
  // Good enough for a token the server doesn't validate.
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    return (c === 'x' ? r : (r & 0x3) | 0x8).toString(16);
  });
}

/// Run greeting → ping → download against `url` (e.g. ws://localhost:5005).
/// hooks: { log, ping, download, onProgress }
export async function runMeasurement(url, hooks = {}) {
  const log = hooks.log || (() => {});
  const ws = new WebSocket(url);
  const io = new RmbtIo(ws);
  await io.open();
  log(`connected ${url}`);

  // ---- GREETING ----
  const version = await io.readLine();
  log(`greeting: ${version}`);
  await io.readUntil('ACCEPT TOKEN'); // "ACCEPT TOKEN QUIT"
  io.send(`TOKEN ${uuid4()}_dummy\n`);

  let chunkSize = 4096;
  for (;;) {
    const l = await io.readLine();
    if (l.startsWith('CHUNKSIZE')) chunkSize = parseInt(l.trim().split(/\s+/)[1], 10);
    if (l.startsWith('ACCEPT')) break; // command prompt "ACCEPT GETCHUNKS GETTIME ..."
  }
  log(`token accepted, chunkSize=${chunkSize}`);

  // ---- PING (5 samples; RTT = client PING->PONG) ----
  const rtts = [];
  for (let i = 0; i < 5; i++) {
    if (i > 0) await io.readUntil('ACCEPT'); // ready prompt before each command
    const t0 = perfNow();
    io.send('PING\n');
    await io.readUntil('PONG');
    const t1 = perfNow();
    io.send('OK\n');
    await io.readUntil('TIME');
    rtts.push(t1 - t0);
  }
  const pingMs = Math.min(...rtts);
  log(`ping: ${pingMs.toFixed(2)} ms (samples ${rtts.map((r) => r.toFixed(1)).join(', ')})`);
  if (hooks.ping) hooks.ping(pingMs);

  // ---- DOWNLOAD (GETTIME) ----
  await io.readUntil('ACCEPT'); // prompt after last ping
  const durationSec = 2;
  io.send(`GETTIME ${durationSec} ${chunkSize}\n`);
  const dl = await io.download(chunkSize, hooks.onProgress);
  io.send('OK\n');
  const timeLine = await io.readUntil('TIME'); // "TIME <ns>"
  const serverNs = parseInt(timeLine.trim().split(/\s+/)[1], 10);
  const secs = serverNs > 0 ? serverNs / 1e9 : dl.elapsed;
  const mbps = (dl.bytes * 8) / secs / 1e6;
  log(`download: ${mbps.toFixed(2)} Mbit/s (${(dl.bytes / 1e6).toFixed(1)} MB in ${secs.toFixed(2)} s)`);
  if (hooks.download) hooks.download(mbps, dl.bytes, secs);

  try { ws.close(); } catch (_) {}
  return { pingMs, downloadMbps: mbps, downloadBytes: dl.bytes };
}
