<script lang="ts">
  // Fenêtre flottante de dictée (label « quick »).
  // M2 : enregistrement réel. Aucune transcription — le WAV est la seule
  // sortie, et son chemin est affiché après l'arrêt (pas de faux succès).
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  type Etat = "prêt" | "enregistre" | "pause" | "erreur";

  let etat = $state<Etat>("prêt");
  let niveau = $state(0);
  let secondes = $state(0);
  let message = $state("");
  let timer: ReturnType<typeof setInterval> | null = null;
  let desabonner: (() => void) | null = null;

  onMount(async () => {
    desabonner = await listen<{ rms: number }>("capture:level", (e) => {
      // Échelle en décibels : un VU-mètre linéaire reste collé à zéro avec un
      // micro à faible couplage. -70 dB → 0 %, -20 dB → 100 %.
      const db = 20 * Math.log10(Math.max(e.payload.rms, 1e-6));
      niveau = Math.max(0, Math.min(1, (db + 70) / 50));
    });
  });

  onDestroy(() => {
    desabonner?.();
    if (timer) clearInterval(timer);
  });

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
        const info = await invoke<{
          path: string;
          duration_secs: number;
          samples: number;
          sample_rate: number;
          channels: number;
        }>("stop_capture");
        etat = "prêt";
        niveau = 0;
        message = `${info.duration_secs.toFixed(2)} s — ${info.samples} échantillons — ${info.path}`;
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
    const m = Math.floor(s / 60);
    return `${m}:${String(s % 60).padStart(2, "0")}`;
  }

  const libelle = $derived(
    etat === "enregistre" ? "● Enregistrement" : etat === "pause" ? "Pause" : etat === "erreur" ? "Erreur" : "Prêt",
  );
</script>

<main>
  <div class="gauche">
    <p class="etat" class:rouge={etat === "enregistre"} aria-live="polite">
      {libelle}
      {#if etat === "enregistre" || etat === "pause"}<span class="chrono">{mmss(secondes)}</span>{/if}
    </p>
    <div class="vu" aria-hidden="true">
      <div class="barre" style="width: {(niveau * 100).toFixed(0)}%"></div>
    </div>
    {#if message}
      <p class="message" class:erreur={etat === "erreur"} title={message}>{message}</p>
    {/if}
  </div>

  <div class="droite">
    {#if etat === "enregistre" || etat === "pause"}
      <button class="secondaire" onclick={basculerPause}>
        {etat === "pause" ? "Reprendre" : "Pause"}
      </button>
    {/if}
    <button class="principal" class:actif={etat === "enregistre" || etat === "pause"} onclick={basculer}>
      {etat === "enregistre" || etat === "pause" ? "Arrêter" : "Enregistrer"}
    </button>
  </div>
</main>

<style>
  :global(body) {
    font-family: system-ui, sans-serif;
    background: #1c1c22;
    color: #f0f0f4;
    margin: 0;
    user-select: none;
  }

  main {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem 1.25rem;
    height: 100vh;
    box-sizing: border-box;
  }

  .gauche {
    min-width: 0;
    flex: 1;
  }

  .etat {
    margin: 0 0 0.5rem;
    font-size: 1rem;
  }

  .etat.rouge {
    color: #ff6b6b;
  }

  .chrono {
    margin-left: 0.5rem;
    font-variant-numeric: tabular-nums;
    color: #c9c9d4;
  }

  .vu {
    height: 8px;
    border-radius: 999px;
    background: #2a2a33;
    overflow: hidden;
  }

  .barre {
    height: 100%;
    background: #4ade80;
    transition: width 60ms linear;
  }

  .message {
    margin: 0.5rem 0 0;
    font-size: 0.75rem;
    color: #9a9aa8;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .message.erreur {
    color: #ff8f8f;
    white-space: normal;
  }

  .droite {
    display: flex;
    gap: 0.5rem;
    flex-shrink: 0;
  }

  button {
    font: inherit;
    padding: 0.55rem 1rem;
    border-radius: 999px;
    border: 1px solid #55555f;
    background: #2a2a33;
    color: #e8e8ee;
    cursor: pointer;
  }

  button.principal {
    background: #2d6cdf;
    border-color: #2d6cdf;
    color: #fff;
  }

  button.principal.actif {
    background: #b91c1c;
    border-color: #b91c1c;
  }

  button.secondaire {
    color: #c9c9d4;
  }
</style>
