// Headless check of the browser RMBTws client using Node 22's global WebSocket
// (same API as the browser). Usage: node browser-poc/node-test.mjs [ws-url]
import { runMeasurement } from './rmbt-ws.js';

const url = process.argv[2] || 'ws://localhost:5005';
console.log(`--- RMBTws PoC against ${url} ---`);

try {
  const r = await runMeasurement(url, {
    log: (m) => console.log('[log]', m),
    onProgress: (bytes, secs) => {
      if (secs > 0) process.stdout.write(`\r[dl] ${(bytes / 1e6).toFixed(1)} MB, ${((bytes * 8) / secs / 1e6).toFixed(0)} Mbit/s   `);
    },
  });
  console.log('\n--- RESULT ---');
  console.log(JSON.stringify(r, null, 2));
  process.exit(0);
} catch (e) {
  console.error('\nFAILED:', e.message);
  process.exit(1);
}
