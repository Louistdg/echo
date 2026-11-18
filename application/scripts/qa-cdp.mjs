#!/usr/bin/env node
/**
 * Local QA for the owned Écho v2.1 WebView2 process only.
 * Start Écho with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9224.
 * This script neither opens a browser nor brings a window to the foreground.
 *
 * Usage (Node 24, from the application directory):
 *   node scripts/qa-cdp.mjs list
 *   node scripts/qa-cdp.mjs eval main scripts/scenario.js
 *   node scripts/qa-cdp.mjs screenshot main docs/qa/notes.png
 *
 * An eval file is an async FUNCTION BODY. `invoke` is provided locally:
 *   const settings = await invoke('settings_get');
 *   return { workspace: settings.workspace, title: document.title };
 * Use `return` for the value printed by the harness. Throws/rejected promises
 * produce a nonzero exit status, with their real JavaScript exception details.
 * Evaluation may mutate this app through its public API; prepare the scenario
 * deliberately. A timeout disconnects QA; it DOES NOT cancel app-side work.
 *
 * Screenshots capture the existing viewport at its actual device scale, without
 * resizing, emulation, scrolling, or activating the window. The explicit compact
 * command temporarily emulates a 750x680 CSS viewport and then clears it.
 * It does not resize the native window. Output includes the
 * PNG's real pixel dimensions and the CSS viewport size.
 */
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve, dirname, basename } from 'node:path';

const ENDPOINT = 'http://127.0.0.1:9224';
const TIMEOUT_MS = 30_000;
const LABELS = new Set(['main', 'quick', 'capture', 'retouche', 'settings']);
const USAGE = 'Usage: node scripts/qa-cdp.mjs list | eval <main|quick|capture|retouche|settings> <script.js> | screenshot <label> <image.png> | compact <label> <image.png>';
const deadline = Date.now() + TIMEOUT_MS;
const remaining = () => Math.max(1, deadline - Date.now());

function allowedTarget(target) {
  if (target?.type !== 'page' || typeof target.webSocketDebuggerUrl !== 'string') return false;
  let url;
  try { url = new URL(target.url); } catch { return false; }
  const tauri = ['http:', 'https:'].includes(url.protocol) && url.hostname === 'tauri.localhost';
  const tauriScheme = url.protocol === 'tauri:' && url.hostname === 'localhost';
  const title = String(target.title || '').normalize('NFD').replace(/\p{Diacritic}/gu, '');
  const echoTitle = /\becho\s+(?:v\s*)?2\.1\b/i.test(title);
  const localLoadingPage = target.url === 'about:blank' || ['127.0.0.1', 'localhost'].includes(url.hostname);
  return tauri || tauriScheme || (echoTitle && localLoadingPage);
}

function ownedWebSocket(raw) {
  const url = new URL(raw);
  if (url.protocol !== 'ws:' || !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || url.port !== '9224' || !url.pathname.startsWith('/devtools/page/')) {
    throw new Error('Refused a debugger socket outside the owned 127.0.0.1:9224 page endpoint.');
  }
  url.hostname = '127.0.0.1';
  url.username = ''; url.password = ''; url.hash = '';
  return url.href;
}

class CDP {
  socket;
  sequence = 0;
  pending = new Map();
  constructor(socket) {
    this.socket = socket;
    socket.addEventListener('message', event => {
      let response;
      try { response = JSON.parse(String(event.data)); } catch { return; }
      const pending = this.pending.get(response.id);
      if (!pending) return;
      this.pending.delete(response.id); clearTimeout(pending.timer);
      if (response.error) pending.reject(new Error(`${pending.method}: ${response.error.message} (${response.error.code})`));
      else pending.resolve(response.result || {});
    });
    socket.addEventListener('close', () => this.rejectAll(new Error('The Écho debugger connection closed.')));
    socket.addEventListener('error', () => this.rejectAll(new Error('The Écho debugger connection failed.')));
  }
  static async connect(target) {
    const socket = new WebSocket(ownedWebSocket(target.webSocketDebuggerUrl));
    const client = new CDP(socket);
    try {
      await new Promise((accept, reject) => {
        const timer = setTimeout(() => { socket.close(); reject(new Error('Timed out connecting to the Écho page.')); }, remaining());
        socket.addEventListener('open', () => { clearTimeout(timer); accept(); }, { once: true });
        socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('Cannot connect to the Écho page debugger.')); }, { once: true });
        socket.addEventListener('close', () => { clearTimeout(timer); reject(new Error('The Écho page closed before connecting.')); }, { once: true });
      });
      return client;
    } catch (error) { client.close(); throw error; }
  }
  call(method, params = {}) {
    return new Promise((accept, reject) => {
      const id = ++this.sequence;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Timed out after 30 seconds: ${method}. App-side work may still be running.`));
      }, remaining());
      this.pending.set(id, { resolve: accept, reject, timer, method });
      try { this.socket.send(JSON.stringify({ id, method, params })); }
      catch (error) { clearTimeout(timer); this.pending.delete(id); reject(error); }
    });
  }
  rejectAll(error) {
    for (const value of this.pending.values()) { clearTimeout(value.timer); value.reject(error); }
    this.pending.clear();
  }
  close() { this.rejectAll(new Error('QA connection closed.')); this.socket.close(); }
}

function evaluateResult(response) {
  if (response.exceptionDetails) {
    const details = response.exceptionDetails;
    const exception = details.exception;
    const error = new Error(exception?.description || (exception?.value !== undefined ? String(exception.value) : details.text || 'JavaScript evaluation failed.'));
    error.name = 'AppEvaluationError';
    error.details = details;
    throw error;
  }
  const result = response.result || {};
  if (result.subtype === 'error') throw new Error(result.description || 'The app returned a JavaScript error.');
  return {
    type: result.type || 'undefined',
    value: Object.hasOwn(result, 'value') ? result.value : null,
    ...(result.unserializableValue ? { unserializableValue: result.unserializableValue } : {}),
    ...(result.description && !Object.hasOwn(result, 'value') ? { description: result.description } : {}),
  };
}

async function identity(client) {
  const result = evaluateResult(await client.call('Runtime.evaluate', {
    expression: `(() => {
      const api = window.__TAURI_INTERNALS__;
      if (!api || typeof api.invoke !== 'function') return null;
      const label = api.metadata?.currentWindow?.label || document.body?.dataset.window;
      return {label, title: document.title, url: location.href};
    })()`,
    returnByValue: true, awaitPromise: true, timeout: remaining(),
  }));
  return result.value && LABELS.has(result.value.label) ? result.value : null;
}

async function discover() {
  const response = await fetch(`${ENDPOINT}/json/list`, { signal: AbortSignal.timeout(remaining()), redirect: 'error' });
  if (!response.ok) throw new Error(`Écho debugger returned HTTP ${response.status}.`);
  const targets = await response.json();
  if (!Array.isArray(targets)) throw new Error('The owned endpoint did not return a CDP target list.');
  const accepted = targets.filter(allowedTarget);
  const inspected = await Promise.all(accepted.map(async target => {
    const client = await CDP.connect(target);
    try {
      const info = await identity(client);
      if (!info) return null;
      return { ...info, id: target.id, target };
    } finally { client.close(); }
  }));
  return inspected.filter(Boolean);
}

async function main() {
  const [command, label, file, ...extra] = process.argv.slice(2);
  if (!command || ['--help', '-h', 'help'].includes(command)) { console.log(USAGE); return; }
  if (!['list', 'eval', 'screenshot', 'compact'].includes(command) || extra.length || (command === 'list' ? Boolean(label || file) : !LABELS.has(label) || !file)) throw new Error(USAGE);
  const targets = await discover();
  if (command === 'list') {
    console.log(JSON.stringify({ endpoint: ENDPOINT, targets: targets.map(({ target, ...info }) => info) }, null, 2));
    return;
  }
  const matches = targets.filter(target => target.label === label);
  if (matches.length !== 1) throw new Error(matches.length ? `More than one Écho page has label '${label}'; refusing an ambiguous target.` : `No verified Écho page has label '${label}'. Is the app ready?`);
  const owned = matches[0];
  const client = await CDP.connect(owned.target);
  try {
    const confirmed = await identity(client);
    if (!confirmed || confirmed.label !== label) throw new Error('The selected Écho page changed before the command.');
    if (command === 'eval') {
      const filename = resolve(file);
      const body = await readFile(filename, 'utf8');
      const expression = '(async () => {\nconst invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);\n' + body + '\n})()\n//# sourceURL=echo-v3-qa/' + encodeURIComponent(basename(filename));
      const result = evaluateResult(await client.call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, timeout: remaining(), userGesture: false }));
      console.log(JSON.stringify({ label, script: filename, ...result }, null, 2));
    } else {
      const compact = command === 'compact';
      if (compact) await client.call('Emulation.setDeviceMetricsOverride', { width: 750, height: 680, deviceScaleFactor: 1, mobile: false });
      try {
      if (compact) await client.call('Runtime.evaluate', { expression: 'new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))', awaitPromise: true, timeout: remaining() });
      const metrics = await client.call('Page.getLayoutMetrics');
      const screenshot = await client.call('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false });
      const png = Buffer.from(screenshot.data, 'base64');
      if (png.length < 24 || png.subarray(0, 8).toString('hex') !== '89504e470d0a1a0a') throw new Error('CDP did not return a valid PNG.');
      const filename = resolve(file);
      await mkdir(dirname(filename), { recursive: true });
      await writeFile(filename, png);
      const viewport = metrics.cssVisualViewport || metrics.visualViewport;
      console.log(JSON.stringify({ label, method: compact ? 'CDP CSS viewport 750x680; native window unchanged' : 'existing native WebView viewport', path: filename, width: png.readUInt32BE(16), height: png.readUInt32BE(20), bytes: png.length, cssViewport: viewport ? { width: viewport.clientWidth, height: viewport.clientHeight, scale: viewport.scale } : null }, null, 2));
      } finally { if (compact) await client.call('Emulation.clearDeviceMetricsOverride'); }
    }
  } finally { client.close(); }
}

const watchdog = setTimeout(() => {
  console.error(JSON.stringify({ error: 'QA command exceeded 30 seconds. App-side work may still be running.' }));
  process.exit(1);
}, TIMEOUT_MS);
watchdog.unref();
try { await main(); }
catch (error) {
  console.error(JSON.stringify({ error: error.message || String(error), name: error.name, ...(error.details ? { exceptionDetails: error.details } : {}) }, null, 2));
  process.exitCode = 1;
} finally { clearTimeout(watchdog); }