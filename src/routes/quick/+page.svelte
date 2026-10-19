<script lang="ts">
  // Fenêtre flottante de dictée (label « quick »).
  // M2 : enregistrement réel, rangé dans <notes>/.echo/audio/.
  // Panneau déplaçable (barre de glissement), épinglable, trois thèmes.
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  type Etat = "prêt" | "enregistre" | "pause" | "erreur";
  type Theme = "nuit" | "rose" | "clair";

  let etat = $state<Etat>("prêt");
  let niveau = $state(0);
  let secondes = $state(0);
  let message = $state("");
  let theme = $state<Theme>("nuit");
  let epingle = $state(true);
  let timer: ReturnType<typeof setInterval> | null = null;
  let desabonner: (() => void) | null = null;

  onMount(async () => {
    const t = localStorage.getItem("echo.theme") as Theme | null;
    if (t === "nuit" || t === "rose" || t === "clair") theme = t;
    const p = localStorage.getItem("echo.epingle");
    epingle = p === null ? true : p === "1";

    desabonner = await listen<{ rms: number }>("capture:level", (e) => {
      const db = 20 * Math.log10(Math.max(e.payload.rms, 1e-6));
      niveau = Math.max(0, Math.min(1, (db + 70) / 50));
    });

    try {
      await getCurrentWindow().setAlwaysOnTop(epingle);
    } catch {
      /* hors Tauri */
    }
  });

  onDestroy(() => {
    desabonner?.();
    if (timer) clearInterval(timer);
  });

  function choisirTheme(t: Theme) {
    theme = t;
    localStorage.setItem("echo.theme", t);
  }

  async function basculerEpingle() {
    epingle = !epingle;
    localStorage.setItem("echo.epingle", epingle ? "1" : "0");
    try {
      await getCurrentWindow().setAlwaysOnTop(epingle);
    } catch (e) {
      message = String(e);
    }
  }

  function demarrerCompteur() {
    if (timer) clearInterval(timer);
    timer = setInterval(() => {
      if (etat === "enregistre") secondes += 1;
    }, 1000);
  }

  function arreterCompteur() {
    if (timer) clearInterval(timer);
    timer = null;
  }

  async function basculer() {
    if (etat === "enregistre" || etat === "pause") {
      arreterCompteur();
      try {
        const info = await invoke<{ path: string; duration_secs: number; samples: number }>(
          "stop_capture",
        );
        etat = "prêt";
        niveau = 0;
        message = `${info.duration_secs.toFixed(1)} s — ${info.path.split(/[\\/]/).pop()}`;
      } catch (e) {
        etat = "erreur";
        message = `Arrêt impossible : ${String(e)}`;
      }
    } else {
      try {
        await invoke("start_capture");
        etat = "enregistre";
        niveau = 0;
        secondes = 0;
        message = "";
        demarrerCompteur();
      } catch (e) {
        etat = "erreur";
        message = `Micro indisponible : ${String(e)}`;
      }
    }
  }

  async function basculerPause() {
    if (etat === "enregistre") {
      try {
        await invoke("pause_capture");
        etat = "pause";
      } catch (e) {
        message = String(e);
      }
    } else if (etat === "pause") {
      try {
        await invoke("resume_capture");
        etat = "enregistre";
      } catch (e) {
        message = String(e);
      }
    }
  }

  function mmss(s: number) {
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }
</script>

<main class={theme}>
  <!-- Barre de glissement : c'est elle qui rend la fenêtre déplaçable. -->
  <header data-tauri-drag-region>
    <span class="titre" data-tauri-drag-region>
      {etat === "enregistre" ? "● Enregistrement" : etat === "pause" ? "Pause" : etat === "erreur" ? "Erreur" : "Écho"}
      {#if etat === "enregistre" || etat === "pause"}
        <span class="chrono" data-tauri-drag-region>{mmss(secondes)}</span>
      {/if}
    </span>
    <div class="outils">
      <button class="pastille" class:nuit={theme === "nuit"} onclick={() => choisirTheme("nuit")} aria-label="Thème nuit"></button>
      <button class="pastille" class:rose={theme === "rose"} onclick={() => choisirTheme("rose")} aria-label="Thème rose"></button>
      <button class="pastille" class:clair={theme === "clair"} onclick={() => choisirTheme("clair")} aria-label="Thème clair"></button>
      <button class="epingle" class:actif={epingle} onclick={basculerEpingle} title={epingle ? "Détacher du premier plan" : "Garder au premier plan"}>
        {epingle ? "📌" : "📍"}
      </button>
    </div>
  </header>

  <section class="corps">
    <div class="vu" aria-hidden="true">
      <div class="barre" style="width: {(niveau * 100).toFixed(0)}%"></div>
    </div>
    {#if message}
      <p class="message" title={message}>{message}</p>
    {/if}
  </section>

  <footer>
    {#if etat === "enregistre" || etat === "pause"}
      <button class="secondaire" onclick={basculerPause}>{etat === "pause" ? "Reprendre" : "Pause"}</button>
    {/if}
    <button class="micro" class:actif={etat === "enregistre" || etat === "pause"} onclick={basculer}>
      {etat === "enregistre" || etat === "pause" ? "■" : "🎙"}
    </button>
  </footer>
</main>

<style>
  :global(body) {
    margin: 0;
    user-select: none;
  }

  main {
    height: 100vh;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    font-family: system-ui, sans-serif;
    padding: 0.5rem 0.75rem;
    gap: 0.4rem;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    cursor: move;
    padding-bottom: 0.25rem;
  }

  .titre {
    font-size: 0.85rem;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .chrono {
    margin-left: 0.4rem;
    font-variant-numeric: tabular-nums;
    font-weight: 400;
    opacity: 0.8;
  }

  .outils {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }

  .pastille {
    width: 14px;
    height: 14px;
    padding: 0;
    border-radius: 50%;
    border: 1px solid rgba(128, 128, 128, 0.5);
    cursor: pointer;
  }

  .pastille.nuit { background: #4ade80; }
  .pastille.rose { background: #ff7ab6; }
  .pastille.clair { background: #ffffff; }

  .epingle {
    background: transparent;
    border: none;
    cursor: pointer;
    font-size: 0.85rem;
    padding: 0 0.15rem;
    opacity: 0.55;
  }

  .epingle.actif {
    opacity: 1;
  }

  .corps {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 0.25rem;
  }

  .vu {
    height: 8px;
    border-radius: 999px;
    overflow: hidden;
  }

  .barre {
    height: 100%;
    transition: width 60ms linear;
  }

  .message {
    margin: 0;
    font-size: 0.7rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    opacity: 0.75;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.4rem;
  }

  .micro {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    border: none;
    font-size: 1.1rem;
    cursor: pointer;
  }

  .secondaire {
    font: inherit;
    font-size: 0.75rem;
    padding: 0.35rem 0.7rem;
    border-radius: 999px;
    cursor: pointer;
  }

  /* --- Thème nuit --- */
  main.nuit { background: #1c1c22; color: #f0f0f4; }
  main.nuit .vu { background: #2a2a33; }
  main.nuit .barre { background: #4ade80; }
  main.nuit .micro { background: #2d6cdf; color: #fff; }
  main.nuit .micro.actif { background: #b91c1c; }
  main.nuit .secondaire { background: #2a2a33; color: #e8e8ee; border: 1px solid #55555f; }

  /* --- Thème rose --- */
  main.rose { background: #2a1420; color: #ffe9f3; }
  main.rose .vu { background: #472235; }
  main.rose .barre { background: #ff7ab6; }
  main.rose .micro { background: #ff7ab6; color: #2a1420; }
  main.rose .micro.actif { background: #ff3d7f; color: #fff; }
  main.rose .secondaire { background: #472235; color: #ffe9f3; border: 1px solid #7a3a58; }

  /* --- Thème clair --- */
  main.clair { background: #f6f6f8; color: #1a1a20; }
  main.clair .vu { background: #dcdce6; }
  main.clair .barre { background: #2d6cdf; }
  main.clair .micro { background: #2d6cdf; color: #fff; }
  main.clair .micro.actif { background: #b91c1c; }
  main.clair .secondaire { background: #ffffff; color: #1a1a20; border: 1px solid #c9c9d4; }
  main.clair .pastille.clair { border-color: #8a8a99; }
</style>
