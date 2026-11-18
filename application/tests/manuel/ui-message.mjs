// Vérifie le message affiché par l'interface après un ajout :
// « Enregistré… » quand tout passe, « NON ENREGISTRÉ… » quand l'écriture échoue.
// Le clic est déclenché dans la page (pas d'injection système), mais tout le
// reste est réel : gestionnaire Svelte, invoke Tauri, journal, note.
//
// Prérequis : voir tests/manuel/README.md.
const [mode = "manuel"] = process.argv.slice(2);
const CDP = Number(process.env.ECHO_CDP ?? 9222);

const targets = await (await fetch(`http://127.0.0.1:${CDP}/json`)).json();
const t = targets.find((x) => x.type === "page" && new URL(x.url).pathname === "/");
if (!t) throw new Error("page principale introuvable");

const expr = `(async () => {
  const ta = document.querySelector('textarea');
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set;
  set.call(ta, 'saisie via interface ${mode}');
  ta.dispatchEvent(new Event('input', { bubbles: true }));
  await new Promise(r => setTimeout(r, 120));
  document.querySelector('button.principal').click();
  await new Promise(r => setTimeout(r, 1200));
  const m = document.querySelector('.message');
  return m ? m.innerText : '(aucun message)';
})()`;

const value = await new Promise((resolve, reject) => {
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  const timer = setTimeout(() => reject(new Error("timeout")), 15000);
  ws.onopen = () =>
    ws.send(
      JSON.stringify({
        id: 1,
        method: "Runtime.evaluate",
        params: { expression: expr, awaitPromise: true, returnByValue: true },
      }),
    );
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id !== 1) return;
    clearTimeout(timer);
    ws.close();
    if (m.result?.exceptionDetails) return reject(new Error(m.result.exceptionDetails.text));
    resolve(m.result?.result?.value);
  };
  ws.onerror = () => {
    clearTimeout(timer);
    reject(new Error("connexion perdue"));
  };
});

console.log(JSON.stringify({ mode, message: value }, null, 2));
