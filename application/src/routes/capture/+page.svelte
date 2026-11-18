<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import Scan from 'lucide-svelte/icons/scan';
import X from 'lucide-svelte/icons/x';
import Maximize from 'lucide-svelte/icons/maximize';
  import { rectFromPoints, sourcePoint, type CaptureDocument, type Point } from "$lib/capture/scene";

  let capture = $state<CaptureDocument | null>(null);
  let image = $state<HTMLImageElement>();
  let overlay: HTMLDivElement;
  let start = $state<Point | null>(null);
  let current = $state<Point | null>(null);
  let busy = $state(false);
  let ready = $state(false);
  let error = $state("");
  let request = 0;
  let bounds = $state({ x: 0, y: 0, width: 1, height: 1 });
  const region = $derived(capture && start && current
    ? rectFromPoints(start, current, capture.width, capture.height) : null);

  async function load() {
    const token = ++request;
    busy = false; ready = false; error = ""; start = null; current = null;
    try {
      const value = await invoke<CaptureDocument | null>("capture_current");
      if (token === request) capture = value;
    } catch (e) { if (token === request) error = String(e); }
  }

  function updateBounds() {
    if (!image) return;
    const rect = image.getBoundingClientRect();
    bounds = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
  }

  function point(event: PointerEvent): Point {
    updateBounds();
    return sourcePoint({ x: event.clientX, y: event.clientY }, bounds, capture!.width, capture!.height);
  }

  function begin(event: PointerEvent) {
    if (event.button !== 0 || busy || !ready || !capture || (event.target as Element).closest("button")) return;
    event.preventDefault();
    overlay.setPointerCapture(event.pointerId);
    start = point(event); current = start;
  }

  function move(event: PointerEvent) {
    if (!start || busy || !capture) return;
    current = point(event);
  }

  async function finish(event: PointerEvent) {
    if (!start || busy || !capture) return;
    current = point(event);
    if (overlay.hasPointerCapture(event.pointerId)) overlay.releasePointerCapture(event.pointerId);
    if (!region || region.width < 4 || region.height < 4) { start = null; current = null; return; }
    await select(false);
  }

  async function select(full: boolean) {
    if (!capture || busy || !ready) return;
    const selected = full ? { x: 0, y: 0, width: capture.width, height: capture.height } : region;
    if (!selected) return;
    busy = true; error = "";
    try { await invoke("capture_select", { id: capture.id, ...selected }); }
    catch (e) { error = String(e); busy = false; }
  }

  async function close() {
    if (busy) return;
    try { await invoke("capture_close"); }
    catch (e) { error = String(e); }
  }

  function key(event: KeyboardEvent) {
    if (event.key === "Escape") { event.preventDefault(); void close(); }
    if (event.key === "Enter" && !(event.target instanceof HTMLButtonElement)) {
      event.preventDefault(); void select(!region);
    }
  }

  onMount(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen("capture:ready", () => { void load(); }).then(fn => { if (disposed) fn(); else unlisten = fn; });
    void load();
    window.addEventListener("keydown", key);
    window.addEventListener("resize", updateBounds);
    return () => { disposed = true; ++request; unlisten?.(); window.removeEventListener("keydown", key); window.removeEventListener("resize", updateBounds); };
  });
</script>

<div class="selection-screen" bind:this={overlay} role="application" aria-label="Sélection de la capture d’écran"
  onpointerdown={begin} onpointermove={move} onpointerup={finish}
  onpointercancel={() => { start = null; current = null; }}
  oncontextmenu={event => { event.preventDefault(); void close(); }}>
  {#if capture}
    <img class="screen-image" bind:this={image} src={capture.source} alt="Écran à capturer" draggable="false"
      onload={() => { ready = true; updateBounds(); }} />
  {/if}
  {#if region && capture}
    {@const left = bounds.x + region.x / capture.width * bounds.width}
    {@const top = bounds.y + region.y / capture.height * bounds.height}
    {@const width = region.width / capture.width * bounds.width}
    {@const height = region.height / capture.height * bounds.height}
    <div class="selection-box" style:left={`${left}px`} style:top={`${top}px`} style:width={`${width}px`} style:height={`${height}px`}>
      <span class="selection-size">{region.width} × {region.height} px</span>
    </div>
  {:else}
    <div class="shade"></div>
  {/if}
  <div class="selection-toolbar">
    <Scan size={20} aria-hidden="true" />
    <div class="instruction"><strong>{busy ? "Ouverture de l’éditeur…" : "Sélectionne une zone"}</strong>
      <span>Glisse sur l’écran · Échap pour fermer</span></div>
    <button class="screen-button" onclick={() => select(true)} disabled={!ready || busy} title="Capturer tout l’écran (Entrée)">
      <Maximize size={17} aria-hidden="true" /> Tout l’écran
    </button>
    <button class="screen-button close-button" onclick={close} disabled={busy} aria-label="Annuler la capture" title="Annuler (Échap)"><X size={20} /></button>
  </div>
  {#if error}<div class="capture-error" role="alert">{error}<button class="screen-button" onclick={load}>Réessayer</button></div>{/if}
  {#if !capture && !error}<div class="loading-message" role="status">Préparation de la capture…</div>{/if}
</div>

<style>
  :global(html), :global(body) { margin: 0; overflow: hidden; height: 100%; }
  .selection-screen { position: fixed; inset: 0; cursor: crosshair; touch-action: none; user-select: none; background: #151821; color: #fff; }
  .screen-image { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: fill; pointer-events: none; }
  .shade { position: absolute; inset: 0; background: #11141c66; pointer-events: none; }
  .selection-box { position: fixed; outline: 2px solid #b2a5ff; box-shadow: 0 0 0 20000px #11141c80; pointer-events: none; }
  .selection-size { position: absolute; left: 0; top: 0; padding: 4px 8px; background: #362564; color: #fff; font-size: 12px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .selection-toolbar { position: fixed; display: flex; align-items: center; gap: 16px; top: 24px; left: 50%; transform: translateX(-50%); padding: 12px 14px 12px 18px; background: #242330; box-shadow: 0 8px 30px #0005; border-radius: 10px; cursor: default; max-width: calc(100vw - 32px); }
  .instruction { display: grid; gap: 3px; white-space: nowrap; }
  .instruction strong { font-size: 14px; font-weight: 600; }
  .instruction span { font-size: 12px; color: #d5d3df; }
  .screen-button { display: inline-flex; align-items: center; justify-content: center; gap: 7px; min-height: 36px; padding: 0 11px; font: inherit; font-size: 13px; white-space: nowrap; color: #fff; background: #3a384a; border: 1px solid #686375; border-radius: 6px; cursor: pointer; }
  .screen-button:hover:not(:disabled) { background: #504a67; }
  .screen-button:focus-visible { outline: 3px solid #c9baff; outline-offset: 3px; }
  .screen-button:disabled { opacity: .55; cursor: wait; }
  .close-button { padding: 0; width: 36px; border-color: transparent; background: transparent; }
  .capture-error { position: fixed; display: flex; align-items: center; gap: 16px; bottom: 28px; left: 50%; transform: translateX(-50%); padding: 14px 18px; max-width: min(700px, calc(100vw - 32px)); border-radius: 8px; background: #682a31; box-shadow: 0 6px 24px #0005; font-size: 14px; cursor: default; }
  .loading-message { position: fixed; top: 50%; left: 50%; transform: translate(-50%,-50%); font-size: 15px; }
  @media (max-width: 650px) { .selection-toolbar { top: 12px; gap: 9px; padding: 10px; } .instruction span { display: none; } .instruction strong { white-space: normal; } }
</style>
