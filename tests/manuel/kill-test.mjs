// Test décisif M1 : 100 ajouts avec arrêts brutaux aléatoires.
// Chaque entrée est confirmée par une réponse de l'application (donc journal +
// note écrits). Critère final : chaque texte exactement une fois, aucun doublon,
// note lisible. Un arrêt entre journal et note est rattrapé par le rejeu ; un
// arrêt après l'écriture de la note ne produit pas de doublon (marqueur id).
//
// Prérequis : voir tests/manuel/README.md (Vite sur 1420, CDP sur 9222,
// settings.json pointant vers ECHO_NOTES).
import { spawn, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const BASE = process.env.ECHO_BASE ?? process.cwd();
const EXE = process.env.ECHO_EXE ?? path.join(BASE, "src-tauri", "target", "debug", "echo.exe");
const NOTES = process.env.ECHO_NOTES ?? path.join(BASE, ".test-notes");
const CDP = Number(process.env.ECHO_CDP ?? 9222);
const NOTE = path.join(NOTES, new Date().toISOString().slice(0, 10) + ".md");
const N = 100;
const TAUX_ARRET = 0.2;

let app = null;
let arrets = 0;
const incidents = [];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function cibles() {
  const r = await fetch(`http://127.0.0.1:${CDP}/json`, { signal: AbortSignal.timeout(1200) });
  return r.json();
}

async function ciblePrincipale() {
  const t = (await cibles()).find((x) => x.type === "page" && new URL(x.url).pathname === "/");
  if (!t) throw new Error("pas de page principale");
  return t;
}

function evaluer(expr, timeoutMs = 5000) {
  return new Promise((resolve, reject) => {
    ciblePrincipale()
      .then((t) => {
        const ws = new WebSocket(t.webSocketDebuggerUrl);
        const timer = setTimeout(() => {
          try { ws.close(); } catch {}
          reject(new Error("timeout"));
        }, timeoutMs);
        ws.onopen = () =>
          ws.send(JSON.stringify({
            id: 1,
            method: "Runtime.evaluate",
            params: { expression: expr, awaitPromise: true, returnByValue: true },
          }));
        ws.onmessage = (ev) => {
          const m = JSON.parse(ev.data);
          if (m.id !== 1) return;
          clearTimeout(timer);
          ws.close();
          if (m.result?.exceptionDetails) {
            return reject(new Error("exception: " + m.result.exceptionDetails.text));
          }
          resolve(m.result?.result?.value);
        };
        ws.onerror = () => {
          clearTimeout(timer);
          reject(new Error("connexion perdue"));
        };
      })
      .catch(reject);
  });
}

async function pret() {
  try {
    return (await evaluer("typeof window.__TAURI_INTERNALS__")) === "object";
  } catch {
    return false;
  }
}

async function demarrer() {
  if (app && app.exitCode === null && (await pret())) return;
  if (app) arreter();
  app = spawn(EXE, [], {
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP}` },
    stdio: "ignore",
    windowsHide: true,
  });
  app.on("exit", () => { app = null; });
  for (let i = 0; i < 200; i++) {
    if (await pret()) { await sleep(250); return; }
    await sleep(100);
  }
  throw new Error("l'application ne démarre pas");
}

function arreter() {
  if (!app) return;
  spawnSync("taskkill", ["/F", "/IM", "echo.exe"], { stdio: "ignore" });
  arrets++;
  app = null;
}

async function attendreDisparition() {
  for (let i = 0; i < 40; i++) {
    try { await cibles(); } catch { return; }
    await sleep(100);
  }
}

function lireNote() {
  try { return readFileSync(NOTE, "utf8"); } catch { return ""; }
}

async function ajouter(texte) {
  const expr =
    "window.__TAURI_INTERNALS__.invoke('append_to_today_note',{text:" +
    JSON.stringify(texte) +
    "}).then(r=>'OK:'+r.id).catch(e=>'ERR:'+String(e))";
  return evaluer(expr);
}

await demarrer();

for (let i = 1; i <= N; i++) {
  const texte = `entrée ${String(i).padStart(3, "0")}`;
  let confirme = false;

  for (let essai = 0; essai < 8 && !confirme; essai++) {
    await demarrer();
    const arretProgramme = Math.random() < TAUX_ARRET;
    const p = ajouter(texte).catch((e) => "ERR:" + e.message);
    if (arretProgramme) setTimeout(arreter, Math.floor(Math.random() * 25));
    const r = await p;

    if (typeof r === "string" && r.startsWith("OK:")) {
      confirme = true;
    } else {
      incidents.push({ texte, essai, resultat: String(r ?? "VIDE").slice(0, 120) });
      await sleep(150);
      arreter();
      await attendreDisparition();
      await demarrer();
      if (lireNote().includes(texte)) confirme = true; // rejeu réussi
    }
  }

  if (!confirme) {
    console.log(JSON.stringify({ echec: "entrée non confirmée", texte, incidents }, null, 2));
    process.exit(1);
  }
}

arreter();
await attendreDisparition();

const contenu = lireNote();
const manquantes = [];
const doubles = [];
for (let i = 1; i <= N; i++) {
  const texte = `entrée ${String(i).padStart(3, "0")}`;
  const n = contenu.split(texte).length - 1;
  if (n === 0) manquantes.push(texte);
  if (n > 1) doubles.push([texte, n]);
}
const marqueurs = contenu.split("<!--echo:").length - 1;

const rapport = {
  entrees_attendues: N,
  arrets,
  incidents: incidents.length,
  manquantes,
  doubles,
  marqueurs,
  taille_note: contenu.length,
  ok: manquantes.length === 0 && doubles.length === 0 && marqueurs === N,
};
console.log(JSON.stringify(rapport, null, 2));
writeFileSync(path.join(BASE, "kill-test-rapport.json"), JSON.stringify({ rapport, incidents }, null, 2));
process.exit(rapport.ok ? 0 : 2);
