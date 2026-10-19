// Suite M2 : mesure l'enregistrement réel (WAV, niveaux, pause, purge).
// Usage : node m2-suite.mjs <ten|pause|kill|rms>
import { spawn, spawnSync } from "node:child_process";
import { readFileSync, readdirSync, existsSync, writeFileSync } from "node:fs";
import path from "node:path";

const BASE = process.env.ECHO_BASE ?? process.cwd();
const EXE = process.env.ECHO_EXE ?? path.join(BASE, "src-tauri", "target", "debug", "echo.exe");
const CDP = Number(process.env.ECHO_CDP ?? 9222);
const TEMP = path.join(process.env.LOCALAPPDATA, "Temp", "Écho");
const TON = path.join(process.env.TEMP, "echo-ton-test.wav");
const mode = process.argv[2] ?? "ten";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let app = null;

async function cible() {
  const t = await (await fetch(`http://127.0.0.1:${CDP}/json`)).json();
  const pages = t.filter((x) => x.type === "page");
  // Les deux fenêtres chargent la même SPA : on les distingue par le label
  // Tauri, pas par l'URL (la fenêtre « quick » navigue après le montage).
  for (const p of pages) {
    try {
      const label = await evalSur(p, "window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label", 3000);
      if (label === "main") return p;
    } catch { /* cible pas encore prête */ }
  }
  throw new Error("fenêtre principale introuvable");
}

function evalSur(t, expr, timeoutMs = 120000) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(t.webSocketDebuggerUrl);
    const timer = setTimeout(() => { try { ws.close(); } catch {} reject(new Error("timeout")); }, timeoutMs);
    ws.onopen = () => ws.send(JSON.stringify({
      id: 1, method: "Runtime.evaluate",
      params: { expression: expr, awaitPromise: true, returnByValue: true },
    }));
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      if (m.id !== 1) return;
      clearTimeout(timer); ws.close();
      if (m.result?.exceptionDetails) {
        const d = m.result.exceptionDetails;
        return reject(new Error(`${d.text} ${d.exception?.description ?? ""}`));
      }
      resolve(m.result?.result?.value);
    };
    ws.onerror = () => { clearTimeout(timer); reject(new Error("connexion perdue")); };
  });
}

async function evaluer(expr, timeoutMs = 120000) {
  return evalSur(await cible(), expr, timeoutMs);
}

async function pret() {
  try { return (await evaluer("typeof window.__TAURI_INTERNALS__", 3000)) === "object"; } catch { return false; }
}

async function demarrer() {
  if (app && app.exitCode === null && (await pret())) return;
  if (app) arreter();
  // Tuer une instance résiduelle et attendre la fermeture du port : sinon on
  // interroge la page d'un processus déjà mort (cible CDP zombie).
  spawnSync("taskkill", ["/F", "/IM", "echo.exe"], { stdio: "ignore" });
  for (let i = 0; i < 40; i++) {
    try { await cibles(); await sleep(150); } catch { break; }
  }
  app = spawn(EXE, [], {
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP}` },
    stdio: "ignore", windowsHide: true,
  });
  app.on("exit", () => { app = null; });
  for (let i = 0; i < 150; i++) {
    if (app && app.exitCode === null && (await pret())) { await sleep(400); return; }
    await sleep(120);
  }
  throw new Error("app ne démarre pas");
}

function arreter() {
  if (!app) return;
  spawnSync("taskkill", ["/F", "/IM", "echo.exe"], { stdio: "ignore" });
  app = null;
}

function wav(nom) {
  const p = path.join(TEMP, nom);
  const b = readFileSync(p);
  const u16 = (o) => b.readUInt16LE(o);
  const u32 = (o) => b.readUInt32LE(o);
  return {
    fichier: nom,
    octets: b.length,
    riff: b.toString("ascii", 0, 4),
    wave: b.toString("ascii", 8, 12),
    canaux: u16(22),
    taux: u32(24),
    bits: u16(34),
    data: b.toString("ascii", 36, 40),
    taille_data: u32(40),
    donnees_attendues: b.length - 44,
  };
}

async function installerEcoute() {
  return evaluer(`(async () => {
    window.__levels = [];
    window.__t0 = Date.now();
    const id = window.__TAURI_INTERNALS__.transformCallback((p) => window.__levels.push({ t: Date.now() - window.__t0, rms: p.payload.rms }));
    await window.__TAURI_INTERNALS__.invoke('plugin:event|listen', { event: 'capture:level', target: { kind: 'Any' }, handler: id });
    return 'ok';
  })()`);
}

const INV = "window.__TAURI_INTERNALS__.invoke";
const demarrerCapture = () => evaluer(`(async () => { try { await ${INV}('start_capture'); return 'ok'; } catch (e) { return 'ERR:' + String(e); } })()`);
const arreterCapture = () => evaluer(`(async () => { try { return JSON.stringify(await ${INV}('stop_capture')); } catch (e) { return 'ERR:' + String(e); } })()`);
const pause = () => evaluer(`(async () => { try { await ${INV}('pause_capture'); return 'ok'; } catch (e) { return 'ERR:' + String(e); } })()`);
const reprendre = () => evaluer(`(async () => { try { await ${INV}('resume_capture'); return 'ok'; } catch (e) { return 'ERR:' + String(e); } })()`);
const lireNiveaux = () => evaluer(`JSON.stringify(window.__levels ?? [])`);

const resultat = { mode, verifications: [] };
const noter = (nom, valeur, ok = null) => {
  resultat.verifications.push({ nom, valeur, ok });
  console.log(`${ok === false ? "ECHEC" : ok === true ? "OK   " : "INFO "} ${nom} : ${JSON.stringify(valeur)}`);
};

await demarrer();

if (mode === "ten" || mode === "pause" || mode === "rms") {
  await installerEcoute();
  const demarrage = await demarrerCapture();
  if (demarrage !== "ok") { noter("demarrage capture", demarrage, false); console.log(JSON.stringify(resultat, null, 2)); process.exit(3); }
}

if (mode === "ten") {
  await sleep(10000);
  const n1 = JSON.parse(await lireNiveaux()).length;
  const info = JSON.parse(await arreterCapture());
  noter("duree", info.duration_secs);
  noter("echantillons", info.samples);
  const ecart = Math.abs(info.duration_secs - 10);
  noter("duree proche de 10 s (tolerance 0,25 s)", ecart, ecart < 0.25);
  const w = wav(path.basename(info.path));
  noter("wav", w);
  noter("taille = 44 + 2*echantillons", w.octets === 44 + 2 * info.samples, w.octets === 44 + 2 * info.samples);
  noter("canaux = 1", w.canaux === 1, w.canaux === 1);
  noter("taux = 16000", w.taux === 16000, w.taux === 16000);
  noter("evenements de niveau pendant 10 s", n1);
  // Les événements déjà en file peuvent arriver juste après l'arrêt ; ce qui
  // compte est qu'aucun nouvel événement ne soit produit ensuite.
  await sleep(600);
  const n2 = JSON.parse(await lireNiveaux()).length;
  await sleep(1000);
  const n3 = JSON.parse(await lireNiveaux()).length;
  noter("file vidée après arrêt (n2, n3)", [n1, n2, n3]);
  noter("plus aucun événement ensuite", n3 === n2, n3 === n2);
}
if (mode === "pause") {
  await sleep(2000);
  noter("pause", await pause());
  await sleep(2000);
  noter("reprise", await reprendre());
  await sleep(2000);
  const info = JSON.parse(await arreterCapture());
  noter("duree avec pause", info.duration_secs);
  noter("duree ~ 4 s (2+2 actives, 2 en pause)", Math.abs(info.duration_secs - 4) < 0.4, Math.abs(info.duration_secs - 4) < 0.4);
}
if (mode === "rms") {
  noter("ecoute installee", await evaluer("typeof window.__levels"));
  await sleep(1500);
  noter("apres 1,5 s", await evaluer("JSON.stringify({ type: typeof window.__levels, n: window.__levels?.length })"));
  const fond = JSON.parse(await lireNiveaux());
  const jouer = spawnSync("powershell", ["-NoProfile", "-Command",
    `Add-Type -AssemblyName System.Windows.Forms; (New-Object System.Media.SoundPlayer('${TON}')).PlaySync()`],
    { stdio: "ignore", timeout: 15000 });
  await sleep(500);
  const apres = JSON.parse(await lireNiveaux());
  const info = JSON.parse(await arreterCapture());
  const pdt = apres.slice(fond.length);
  const maxFond = Math.max(...fond.map((l) => l.rms));
  const maxTon = Math.max(...pdt.map((l) => l.rms));
  noter("fermeture du lecteur", jouer.status);
  noter("evenements", apres.length);
  noter("rms max pendant le silence", maxFond);
  noter("rms max pendant le ton", maxTon);
  noter("rapport signal/fond", maxFond > 0 ? Number((maxTon / maxFond).toFixed(1)) : "n/a");
  noter("duree", info.duration_secs);
  writeFileSync(path.join(BASE, "tests", "manuel", "rms-trace.json"), JSON.stringify(apres));
}
if (mode === "kill") {
  await installerEcoute();
  noter("demarrage", await demarrerCapture());
  await sleep(1200);
  arreter();
  await sleep(600);
  const restes = existsSync(TEMP) ? readdirSync(TEMP).filter((f) => f.endsWith(".wav")) : [];
  noter("wav restant apres taskkill", restes);
  noter("au moins un wav laisse (attendu)", restes.length >= 1, restes.length >= 1);
  await sleep(300);
  await demarrer();
  await sleep(1200);
  const apres = existsSync(TEMP) ? readdirSync(TEMP).filter((f) => f.endsWith(".wav")) : [];
  noter("wav restants apres redemarrage", apres);
  noter("purge au demarrage", apres.length === 0, apres.length === 0);
}

console.log(JSON.stringify(resultat, null, 2));
arreter();
process.exit(resultat.verifications.some((v) => v.ok === false) ? 2 : 0);
