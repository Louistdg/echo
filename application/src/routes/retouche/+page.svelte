<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import ArrowUpRight from 'lucide-svelte/icons/arrow-up-right';
import Check from 'lucide-svelte/icons/check';
import Copy from 'lucide-svelte/icons/copy';
import CropIcon from 'lucide-svelte/icons/crop';
import ImageIcon from 'lucide-svelte/icons/image';
import MousePointer2 from 'lucide-svelte/icons/mouse-pointer-2';
import Pencil from 'lucide-svelte/icons/pencil';
import Redo2 from 'lucide-svelte/icons/redo-2';
import Save from 'lucide-svelte/icons/save';
import Scan from 'lucide-svelte/icons/scan';
import Square from 'lucide-svelte/icons/square';
import SquareDashed from 'lucide-svelte/icons/square-dashed';
import Trash2 from 'lucide-svelte/icons/trash-2';
import Type from 'lucide-svelte/icons/type';
import Undo2 from 'lucide-svelte/icons/undo-2';
import X from 'lucide-svelte/icons/x';
import ZoomIn from 'lucide-svelte/icons/zoom-in';
import ZoomOut from 'lucide-svelte/icons/zoom-out';
  import { CaptureCanvas, type Tool } from "$lib/capture/controller";
  import { cloneScene, emptyScene, readScene, SceneHistory, type Annotation, type CaptureDocument, type Crop, type Scene } from "$lib/capture/scene";

  const tools = [
    { id: "select" as Tool, name: "Sélection", icon: MousePointer2, key: "V" },
    { id: "arrow" as Tool, name: "Flèche", icon: ArrowUpRight, key: "A" },
    { id: "rect" as Tool, name: "Rectangle", icon: Square, key: "R" },
    { id: "pen" as Tool, name: "Crayon", icon: Pencil, key: "P" },
    { id: "text" as Tool, name: "Texte", icon: Type, key: "T" },
    { id: "blackout" as Tool, name: "Masquer", icon: SquareDashed, key: "M" },
    { id: "crop" as Tool, name: "Recadrer", icon: CropIcon, key: "C" },
  ];
  const palette = [
    { value: "#d53f4e", name: "Rouge" }, { value: "#2367c3", name: "Bleu" },
    { value: "#2f8358", name: "Vert" }, { value: "#f2c94c", name: "Jaune" },
    { value: "#ffffff", name: "Blanc" }, { value: "#151820", name: "Noir" },
  ];
  const kindNames: Record<string, string> = { text: "Texte", rect: "Rectangle", pen: "Trait au crayon", arrow: "Flèche", blackout: "Masquage" };

  let capture = $state<CaptureDocument | null>(null);
  let scene = $state<Scene | null>(null);
  let tool = $state<Tool>("select");
  let selected = $state<string | null>(null);
  let color = $state("#d53f4e");
  let thickness = $state(4);
  let fontSize = $state(28);
  let pendingCrop = $state<Crop | null>(null);
  let zoom = $state(1);
  let fitted = $state(true);
  let canUndo = $state(false);
  let canRedo = $state(false);
  let busy = $state(false);
  let loading = $state(true);
  let error = $state("");
  let status = $state("Original conservé");
  let revision = 0;
  let savedRevision = 0;
  let loadToken = 0;
  let history: SceneHistory | undefined;
  let canvas: CaptureCanvas | undefined;
  let K: typeof import("konva").default;
  let viewport: HTMLDivElement;
  let canvasHost = $state<HTMLDivElement>();
  let textArea = $state<HTMLTextAreaElement>();
  let editing = $state<Annotation | null>(null);
  let textValue = $state("");
  let draftTimer: ReturnType<typeof setTimeout> | undefined;
  let draftQueue: Promise<void> = Promise.resolve();
  let lastDraftError: unknown = null;
  let activeOperations = 0;
  let operationWaiters: Array<() => void> = [];
  let quitPending = false;
  const currentItem = $derived(scene?.objects.find(item => item.id === selected) ?? null);
  const isCropped = $derived(!!scene && (scene.crop.x !== 0 || scene.crop.y !== 0 || scene.crop.width !== scene.sourceWidth || scene.crop.height !== scene.sourceHeight));
  const editedTextStyle = $derived(editing && scene ?
    `left:${(editing.x - scene.crop.x) * zoom}px;top:${(editing.y - scene.crop.y) * zoom}px;width:${Math.max(80, editing.width * Math.abs(editing.scaleX) * zoom)}px;font-size:${(editing.fontSize ?? 28) * Math.abs(editing.scaleY) * zoom}px;color:${editing.fill ?? editing.stroke};transform:rotate(${editing.rotation}deg)` : "");

  const draftKey = (id: string) => `echo.v3.capture.draft.${id}`;
  function bufferLocally() {
    if (!capture || !scene) return;
    try { localStorage.setItem(draftKey(capture.id), JSON.stringify(scene)); }
    catch { /* Disk remains the authoritative save; its errors are surfaced below. */ }
  }

  function scheduleDraft() {
    bufferLocally();
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = setTimeout(() => { void flushDraft().catch(() => {}); }, 500);
  }

  async function flushDraft() {
    if (draftTimer) { clearTimeout(draftTimer); draftTimer = undefined; }
    if (!capture || !scene || (revision === savedRevision && !lastDraftError)) {
      await draftQueue;
      if (lastDraftError) throw lastDraftError;
      return;
    }
    const id = capture.id;
    const document = JSON.stringify(scene);
    const version = revision;
    status = "Enregistrement du brouillon…";
    const task = draftQueue.catch(() => {}).then(async () => {
      await invoke("capture_draft", { id, document });
      lastDraftError = null;
      if (capture?.id === id) {
        savedRevision = version;
        if (revision === version) {
          status = "Brouillon enregistré";
          try { localStorage.removeItem(draftKey(id)); } catch { /* Not required for the disk save. */ }
        }
      }
    });
    draftQueue = task;
    try { await task; }
    catch (e) {
      lastDraftError = e;
      if (capture?.id === id) { error = `Le brouillon n’a pas été enregistré : ${String(e)}`; status = "Brouillon à enregistrer"; }
      throw e;
    }
  }

  function selectionChanged(id: string | null) {
    selected = id;
    const item = scene?.objects.find(item => item.id === id);
    if (item && item.kind !== "blackout") {
      color = item.kind === "text" ? item.fill ?? item.stroke : item.stroke;
      thickness = item.strokeWidth;
      fontSize = item.fontSize ?? fontSize;
      syncStyle();
    }
  }

  function syncStyle() {
    if (canvas) canvas.style = { stroke: color, strokeWidth: thickness, fontSize };
  }

  function commit(next: Scene) {
    scene = cloneScene(next);
    if (!history) history = new SceneHistory(scene);
    history.commit(scene);
    canUndo = history.canUndo; canRedo = history.canRedo;
    revision++;
    status = "Modifications en cours…";
    canvas?.setScene(scene);
    scheduleDraft();
  }

  async function load() {
    const token = ++loadToken;
    loading = true; error = "";
    try {
      const value = await invoke<CaptureDocument | null>("capture_current");
      if (token !== loadToken) return;
      if (!value) { canvas?.destroy(); canvas = undefined; capture = null; scene = null; loading = false; return; }
      // A readiness event may follow the initial read; do not erase live edits of the same source.
      if (capture?.id === value.id && scene && canvas && capture.source === value.source) { loading = false; return; }
      if (draftTimer) clearTimeout(draftTimer);
      await draftQueue.catch(() => {});
      draftQueue = Promise.resolve();
      if (token !== loadToken) return;
      const bitmap = new window.Image();
      bitmap.src = value.source;
      await bitmap.decode();
      if (bitmap.naturalWidth !== value.width || bitmap.naturalHeight !== value.height) throw new Error("Les dimensions de la capture sont incohérentes.");
      let restored = readScene(value.document, value.width, value.height);
      let recovered = false;
      try {
        const buffered = localStorage.getItem(draftKey(value.id));
        if (buffered && buffered !== value.document) { restored = readScene(buffered, value.width, value.height); recovered = true; }
      } catch { /* An invalid local cache must not replace a valid disk document. */ }
      if (!K) K = (await import("konva")).default;
      if (token !== loadToken) return;
      canvas?.destroy(); canvas = undefined;
      capture = value; scene = restored; selected = null; editing = null; pendingCrop = null;
      history = new SceneHistory(restored); canUndo = false; canRedo = false;
      revision = recovered ? 1 : 0; savedRevision = 0; lastDraftError = null;
      tool = "select"; fitted = true;
      status = recovered ? "Brouillon récupéré" : value.document ? "Brouillon enregistré" : "Original conservé";
      loading = false;
      await tick();
      if (token !== loadToken || !canvasHost) return;
      canvas = new CaptureCanvas(K, canvasHost, bitmap, restored, {
        changed: commit, selected: selectionChanged, editText: startText,
        crop: value => { pendingCrop = value; },
      });
      syncStyle();
      fit();
      if (recovered) scheduleDraft();
      canvasHost.focus({ preventScroll: true });
    } catch (e) { if (token === loadToken) { loading = false; error = String(e); } }
  }

  function chooseTool(value: Tool) {
    if (busy) return;
    finishText(true);
    tool = value;
    pendingCrop = null;
    canvas?.setTool(value);
    if (value !== "select") { selected = null; canvas?.select(null); }
    syncStyle();
  }

  function updateStyle() {
    syncStyle();
    if (!scene || !selected || currentItem?.kind === "blackout") return;
    const next = cloneScene(scene);
    const item = next.objects.find(item => item.id === selected);
    if (!item) return;
    item.stroke = color;
    item.strokeWidth = thickness;
    if (item.kind === "text") { item.fill = color; item.fontSize = fontSize; }
    commit(next);
  }

  function undo() {
    if (busy || !history) return;
    finishText(true);
    const previous = history.undo();
    if (!previous) return;
    scene = previous; selected = null; pendingCrop = null; revision++;
    canvas?.setScene(previous); canvas?.select(null); canUndo = history.canUndo; canRedo = history.canRedo;
    scheduleDraft(); if (fitted) fit();
  }
  function redo() {
    if (busy || !history) return;
    const next = history.redo();
    if (!next) return;
    scene = next; selected = null; pendingCrop = null; revision++;
    canvas?.setScene(next); canvas?.select(null); canUndo = history.canUndo; canRedo = history.canRedo;
    scheduleDraft(); if (fitted) fit();
  }

  function removeSelected() {
    if (!scene || !selected || busy) return;
    const next = cloneScene(scene); next.objects = next.objects.filter(item => item.id !== selected);
    selected = null; commit(next); canvas?.select(null);
  }

  function clearAnnotations() {
    if (!scene || busy) return;
    finishText(false); selected = null;
    const next = cloneScene(scene); next.objects = []; commit(next);
  }

  function cropApply() {
    if (!scene || !pendingCrop) return;
    const next = cloneScene(scene); next.crop = { ...pendingCrop };
    pendingCrop = null; commit(next); chooseTool("select"); fit();
  }
  function cropReset() {
    if (!scene) return;
    const next = cloneScene(scene); next.crop = emptyScene(scene.sourceWidth, scene.sourceHeight).crop;
    pendingCrop = null; commit(next); chooseTool("select"); fit();
  }

  function fit() {
    if (!scene || !viewport || !canvas) return;
    fitted = true;
    zoom = Math.max(.05, Math.min(1, (viewport.clientWidth - 64) / scene.crop.width, (viewport.clientHeight - 64) / scene.crop.height));
    canvas.setZoom(zoom);
  }
  function zoomTo(value: number) {
    finishText(true);
    fitted = false;
    zoom = Math.min(4, Math.max(.05, value));
    canvas?.setZoom(zoom);
  }

  function startText(item: Annotation) {
    if (busy) return;
    finishText(true);
    editing = { ...item };
    textValue = item.text ?? "";
    canvas?.hideText(item.id);
    void tick().then(() => { textArea?.focus(); if (textValue) textArea?.select(); });
  }
  function finishText(accept: boolean) {
    if (!editing) return;
    const item = { ...editing, text: textValue.trimEnd() };
    editing = null;
    canvas?.hideText(null);
    if (!accept || !scene) return;
    const next = cloneScene(scene);
    const index = next.objects.findIndex(value => value.id === item.id);
    if (!item.text?.trim()) {
      if (index >= 0) { next.objects.splice(index, 1); commit(next); }
      return;
    }
    if (index >= 0) next.objects[index] = item; else next.objects.push(item);
    selected = item.id; tool = "select"; canvas?.setTool("select");
    commit(next); canvas?.select(item.id);
  }
  function textKey(event: KeyboardEvent) {
    event.stopPropagation();
    if (event.key === "Escape") { event.preventDefault(); finishText(false); canvasHost?.focus(); }
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); finishText(true); canvasHost?.focus(); }
    if (event.key.toLowerCase() === "s" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void save(false); }
  }

  async function save(copy: boolean) {
    if (!capture || !scene || !canvas || busy) return;
    finishText(true);
    if (pendingCrop) cropApply();
    canvas.setEnabled(false);
    busy = true; error = ""; status = copy ? "Copie et enregistrement…" : "Enregistrement de la capture…";
    activeOperations++;
    const id = capture.id;
    try {
      await flushDraft();
      const document = JSON.stringify(scene);
      const image = canvas.exportPng();
      const saved = await invoke<{ path: string }>("capture_save", { id, image, document, copy });
      status = copy ? "Capture copiée et rangée" : "Capture enregistrée";
      savedRevision = revision;
      try { localStorage.removeItem(draftKey(id)); } catch { /* Source and scene already on disk. */ }
      await invoke("capture_close");
      void saved;
    } catch (e) { error = String(e); status = "La capture reste ouverte"; }
    finally { busy = false; canvas?.setEnabled(true); finishOperation(); }
  }

  async function close() {
    if (busy) return;
    finishText(true);
    if (pendingCrop) cropApply();
    canvas?.setEnabled(false); busy = true;
    activeOperations++;
    try { await flushDraft(); await invoke("capture_close"); }
    catch (e) { error = `La capture reste ouverte pour préserver tes modifications. ${String(e)}`; }
    finally { busy = false; canvas?.setEnabled(true); finishOperation(); }
  }

  function finishOperation() {
    activeOperations = Math.max(0, activeOperations - 1);
    if (!activeOperations) {
      const waiting = operationWaiters; operationWaiters = [];
      for (const resolve of waiting) resolve();
    }
  }

  async function prepareQuit() {
    if (quitPending) return;
    quitPending = true;
    let acknowledged = false;
    try {
      if (activeOperations) await new Promise<void>(resolve => operationWaiters.push(resolve));
      if (capture && scene && canvas) {
        finishText(true);
        if (pendingCrop) cropApply();
        canvas.setEnabled(false); busy = true;
        await flushDraft();
      }
      status = "Fermeture d’Écho…";
      await invoke("app_quit_ready");
      acknowledged = true;
    } catch (e) {
      await invoke("app_quit_abort").catch(() => {});
      status = "Fermeture suspendue";
      error = `Tes retouches doivent être enregistrées avant de quitter. ${String(e)}`;
      try { const window = getCurrentWindow(); await window.show(); await window.setFocus(); } catch { /* The existing editor also keeps the error and unacknowledged draft. */ }
    } finally {
      quitPending = false;
      // Keep an acknowledged surface read-only while the other window finishes saving.
      if (!acknowledged) { busy = false; canvas?.setEnabled(true); }
    }
  }

  function key(event: KeyboardEvent) {
    const target = event.target;
    const inField = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement ||
      target instanceof HTMLSelectElement || (target instanceof HTMLElement && target.isContentEditable);
    if (inField || busy) return;
    if (event.key === "Escape") { event.preventDefault(); void close(); return; }
    if (event.ctrlKey || event.metaKey) {
      const key = event.key.toLowerCase();
      if (key === "z") { event.preventDefault(); event.shiftKey ? redo() : undo(); }
      if (key === "y") { event.preventDefault(); redo(); }
      if (key === "s") { event.preventDefault(); void save(false); }
      if (key === "c") { event.preventDefault(); void save(true); }
      return;
    }
    const canvasFocused = canvasHost && (document.activeElement === canvasHost || canvasHost.contains(document.activeElement));
    if (!canvasFocused) return;
    if (event.key === "Delete" || event.key === "Backspace") { event.preventDefault(); removeSelected(); return; }
    const nextTool = tools.find(value => value.key.toLowerCase() === event.key.toLowerCase());
    if (nextTool && !event.altKey) { event.preventDefault(); chooseTool(nextTool.id); }
  }

  onMount(() => {
    let disposed = false;
    const cleanup: UnlistenFn[] = [];
    const keep = (unlisten: UnlistenFn) => { if (disposed) unlisten(); else cleanup.push(unlisten); };
    void listen("capture:ready", () => { void load(); }).then(keep);
    void listen("app:quit-request", () => { void prepareQuit(); }).then(keep);
    void listen("app:quit-cancelled", () => { quitPending=false;busy=false;canvas?.setEnabled(true);status="Fermeture annulée"; }).then(keep);
    try { void getCurrentWindow().onCloseRequested(event => { event.preventDefault(); void close(); }).then(keep); } catch { /* Browser preview. */ }
    window.addEventListener("keydown", key);
    const observer = new ResizeObserver(() => { if (fitted && !editing) fit(); });
    void load().then(() => { if (viewport && !disposed) observer.observe(viewport); });
    return () => {
      disposed = true; ++loadToken;
      for (const unlisten of cleanup) unlisten();
      window.removeEventListener("keydown", key); observer.disconnect();
      if (draftTimer) clearTimeout(draftTimer);
      bufferLocally(); canvas?.destroy();
    };
  });
</script>

<div class="capture-editor">
  <header class="editor-header">
    <div class="editor-title"><ImageIcon size={23} aria-hidden="true" /><div><h1>Capture d’écran</h1><p>{capture ? "Retouche, puis copie ou enregistre." : "Tes captures, prêtes à annoter."}</p></div></div>
    <div class="header-actions">
      <button class="btn save-button" onclick={() => save(false)} disabled={!scene || busy || loading} title="Enregistrer (Ctrl+S)"><Save size={17} aria-hidden="true" /><span>Enregistrer</span></button>
      <button class="btn primary" onclick={() => save(true)} disabled={!scene || busy || loading} title="Copier et ranger dans le dossier de captures (Ctrl+C)"><Copy size={17} aria-hidden="true" />{busy ? "En cours…" : "Copier et ranger"}</button>
      <button class="icon-btn" onclick={close} disabled={busy} aria-label="Fermer et conserver le brouillon" title="Fermer et conserver le brouillon (Échap)"><X size={20} /></button>
    </div>
  </header>

  <div class="editor-body">
    <aside class="tool-sidebar" aria-label="Outils de retouche">
      <div class="tool-list" role="group" aria-label="Choisir un outil">
        {#each tools as entry}
          <button class="tool-button" class:chosen={tool === entry.id} aria-pressed={tool === entry.id}
            disabled={!scene || busy || loading} title={`${entry.name} (${entry.key})`} onclick={() => chooseTool(entry.id)}>
            <entry.icon size={19} aria-hidden="true" /><span>{entry.name}</span><kbd>{entry.key}</kbd>
          </button>
        {/each}
      </div>
      <div class="tool-properties">
        {#if tool === "crop"}
          <p class="tool-help">Trace un cadre sur l’image. Le recadrage garde l’original intact.</p>
        {:else if tool === "blackout" || currentItem?.kind === "blackout"}
          <p class="tool-help">Le rectangle noir masque la zone dans l’image exportée. L’original reste dans Écho.</p>
        {:else}
          <span class="property-label">Couleur</span>
          <div class="color-list" role="group" aria-label="Couleur de l’annotation">
            {#each palette as swatch}
              <button class="color-swatch" class:chosen={color === swatch.value} style:background-color={swatch.value}
                aria-label={swatch.name} aria-pressed={color === swatch.value} title={swatch.name} disabled={!scene || busy}
                onclick={() => { color = swatch.value; updateStyle(); }}>
                {#if color === swatch.value}<Check size={14} color={swatch.value === "#ffffff" || swatch.value === "#f2c94c" ? "#151820" : "#ffffff"} aria-hidden="true" />{/if}
              </button>
            {/each}
          </div>
          {#if tool === "text" || currentItem?.kind === "text"}
            <label class="property-label" for="capture-font">Taille du texte <span>{fontSize} px</span></label>
            <input id="capture-font" type="range" min="14" max="96" step="2" bind:value={fontSize} onchange={updateStyle} disabled={!scene || busy} />
          {:else}
            <label class="property-label" for="capture-thickness">Épaisseur <span>{thickness} px</span></label>
            <input id="capture-thickness" type="range" min="1" max="16" step="1" bind:value={thickness} onchange={updateStyle} disabled={!scene || busy} />
          {/if}
          {#if tool === "text"}<p class="tool-help">Clique sur l’image pour écrire. Double-clique sur un texte pour le modifier.</p>{/if}
        {/if}
      </div>
      {#if scene && scene.objects.length > 0}
        <div class="annotation-list" aria-label="Annotations de l’image"><h2>Annotations <span>{scene.objects.length}</span></h2>
          {#each [...scene.objects].reverse() as item, i}
            <button class:selected={selected === item.id} disabled={busy} onclick={() => { chooseTool("select"); canvas?.select(item.id); }}
              title={item.kind === "text" ? item.text : kindNames[item.kind]}>{item.kind === "text" ? item.text : `${kindNames[item.kind]} ${scene.objects.length - i}`}</button>
          {/each}
        </div>
      {/if}
      {#if capture}<div class="original-note"><Check size={15} aria-hidden="true" /><span>Original conservé</span></div>{/if}
    </aside>

    <main class="canvas-panel">
      <div class="canvas-toolbar">
        <div class="history-actions" role="group" aria-label="Historique des modifications">
          <button class="icon-btn" onclick={undo} disabled={!canUndo || busy} aria-label="Annuler la dernière modification" title="Annuler (Ctrl+Z)"><Undo2 size={18} /></button>
          <button class="icon-btn" onclick={redo} disabled={!canRedo || busy} aria-label="Rétablir la modification" title="Rétablir (Ctrl+Maj+Z)"><Redo2 size={18} /></button>
          <span class="toolbar-divider"></span>
          <button class="icon-btn" onclick={removeSelected} disabled={!selected || busy} aria-label="Supprimer l’annotation sélectionnée" title="Supprimer l’annotation (Suppr)"><Trash2 size={17} /></button>
        </div>
        <span class="context-help">{tool === "select" ? "Clique sur une annotation pour la déplacer ou la redimensionner." : tools.find(value => value.id === tool)?.name}</span>
        {#if isCropped}<button class="btn ghost restore-crop" onclick={cropReset} disabled={busy}>Rétablir le cadrage</button>{/if}
        <button class="btn ghost clear-annotations" onclick={clearAnnotations} disabled={!scene?.objects.length || busy}>Effacer les annotations</button>
      </div>
      {#if pendingCrop}
        <div class="crop-confirm" role="status"><CropIcon size={17} aria-hidden="true" /><span>{pendingCrop.width} × {pendingCrop.height} px</span><button class="btn primary" onclick={cropApply}>Appliquer le recadrage</button><button class="btn ghost" onclick={() => canvas?.clearCrop()}>Annuler</button></div>
      {/if}
      {#if error}<div class="editor-error" role="alert"><span>{error}</span><button class="btn" onclick={() => { error = ""; void (scene ? flushDraft() : load()).catch(() => {}); }}>Réessayer</button></div>{/if}

      <div class="canvas-scroller" bind:this={viewport}>
        {#if scene}
          <div class="canvas-space">
            <div class="canvas-document">
              <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (This drawing application implements keyboard tools and selection. Its canvas must be focusable; annotation controls also exist in the sidebar.) -->
              <div class="canvas-host" bind:this={canvasHost} tabindex="0" role="application"
                onkeydown={event => { key(event); event.stopPropagation(); }}
                aria-label="Image et annotations. Touches V, A, R, P, T, M et C pour les outils. Suppr efface l’annotation sélectionnée."></div>
              {#if editing}
                <div class="text-edit-wrapper" style={editedTextStyle}>
                  <textarea bind:this={textArea} bind:value={textValue} onkeydown={textKey} aria-label="Texte de l’annotation" placeholder="Écris ici…" spellcheck="true" rows={Math.max(2, textValue.split("\n").length + 1)}></textarea>
                  <div class="text-edit-actions"><button onclick={() => finishText(true)} title="Valider (Ctrl+Entrée)"><Check size={16} aria-hidden="true" />Valider</button><button onclick={() => finishText(false)} aria-label="Annuler la modification du texte"><X size={16} /></button></div>
                </div>
              {/if}
            </div>
          </div>
        {:else}
          <div class="empty-capture"><Scan size={36} strokeWidth={1.4} aria-hidden="true" /><h2>{loading ? "Préparation de l’image…" : "Une capture à retoucher ?"}</h2>
            <p>{loading ? "L’original et ses annotations se chargent." : "Choisis une zone de l’écran. Elle s’ouvrira ici pour être annotée."}</p>
            {#if !loading}<button class="btn primary" onclick={() => invoke("capture_begin").catch(e => { error = String(e); })}>Faire une capture</button>{/if}
          </div>
        {/if}
      </div>

      <footer class="canvas-footer">
        <div class="capture-dimensions">{#if scene}{scene.crop.width} × {scene.crop.height} px <span>PNG</span>{:else}Aucune capture{/if}</div>
        <div class="draft-status" role="status" aria-live="polite">{status}</div>
        <div class="zoom-actions" role="group" aria-label="Zoom de l’image">
          <button class="icon-btn" onclick={() => zoomTo(zoom / 1.25)} disabled={!scene || zoom <= .05 || busy} aria-label="Réduire le zoom"><ZoomOut size={16} /></button>
          <button class="zoom-value" onclick={() => zoomTo(1)} disabled={!scene || busy} title="Afficher à 100 %">{Math.round(zoom * 100)} %</button>
          <button class="icon-btn" onclick={() => zoomTo(zoom * 1.25)} disabled={!scene || zoom >= 4 || busy} aria-label="Augmenter le zoom"><ZoomIn size={16} /></button>
          <button class="btn ghost" onclick={fit} disabled={!scene || busy} title="Adapter l’image à la fenêtre">Ajuster</button>
        </div>
      </footer>
    </main>
  </div>
</div>

<style>
  :global(html), :global(body) { height: 100%; margin: 0; overflow: hidden; }
  .capture-editor { height: 100dvh; display: flex; flex-direction: column; color: var(--text, #242334); background: var(--bg, #f5f6f9); font-family: "Segoe UI Variable", "Segoe UI", sans-serif; font-size: 13px; }
  .editor-header { display: flex; align-items: center; justify-content: space-between; gap: 24px; padding: 17px 22px; background: var(--surface, #fff); border-bottom: 1px solid var(--border, #dedfe7); flex: 0 0 auto; }
  .editor-title { display: flex; align-items: center; gap: 13px; min-width: 0; }
  .editor-title > :global(svg) { color: var(--accent, #6850ac); flex: 0 0 auto; }
  h1 { margin: 0; font-size: 17px; line-height: 1.3; font-weight: 650; letter-spacing: -.015em; }
  .editor-title p { color: var(--muted, #656577); margin: 4px 0 0; font-size: 12px; }
  .header-actions { display: flex; align-items: center; gap: 9px; flex: 0 0 auto; }
  .editor-body { display: grid; grid-template-columns: 178px minmax(0, 1fr); flex: 1; min-height: 0; }
  .tool-sidebar { display: flex; flex-direction: column; min-height: 0; overflow-y: auto; border-right: 1px solid var(--border, #dedfe7); background: var(--sidebar, #f1f2f6); padding: 14px 10px 12px; }
  .tool-list { display: grid; gap: 4px; }
  .tool-button { min-height: 40px; display: flex; align-items: center; gap: 10px; width: 100%; padding: 8px 10px; font: inherit; color: var(--text, #242334); background: transparent; border: 1px solid transparent; border-radius: 7px; text-align: left; cursor: pointer; }
  .tool-button:hover:not(:disabled) { background: var(--accent-soft, #eae5f7); }
  .tool-button.chosen { color: var(--accent, #6850ac); background: var(--accent-soft, #eae5f7); font-weight: 600; }
  .tool-button kbd { margin-left: auto; color: var(--muted, #656577); font: 10px/1.5 "Segoe UI", sans-serif; }
  .tool-button:disabled { opacity: .4; cursor: default; }
  .tool-button:focus-visible, .annotation-list button:focus-visible, .color-swatch:focus-visible { outline: 2px solid var(--accent, #6850ac); outline-offset: 2px; }
  .tool-properties { margin: 18px 6px 0; padding-top: 16px; border-top: 1px solid var(--border, #dedfe7); }
  .property-label { display: flex; justify-content: space-between; gap: 8px; margin: 0 0 9px; font-size: 12px; font-weight: 600; }
  .property-label span { color: var(--muted, #656577); font-weight: 400; font-variant-numeric: tabular-nums; }
  .color-list { display: grid; grid-template-columns: repeat(6, 1fr); gap: 5px; margin-bottom: 20px; }
  .color-swatch { display: grid; place-items: center; width: 20px; height: 24px; padding: 0; border: 1px solid #80808b; border-radius: 4px; cursor: pointer; }
  .color-swatch.chosen { outline: 2px solid var(--accent, #6850ac); outline-offset: 2px; }
  input[type="range"] { accent-color: var(--accent, #6850ac); width: 100%; margin: 1px 0 0; }
  .tool-help { margin: 0 0 10px; color: var(--muted, #656577); font-size: 12px; line-height: 1.6; }
  .tool-properties input + .tool-help { margin-top: 15px; }
  .annotation-list { margin: 20px 6px 0; padding-top: 16px; border-top: 1px solid var(--border, #dedfe7); max-height: 190px; overflow-y: auto; }
  .annotation-list h2 { display: flex; justify-content: space-between; margin: 0 0 8px; font-size: 12px; font-weight: 600; }
  .annotation-list h2 span { color: var(--muted, #656577); font-weight: 400; }
  .annotation-list button { display: block; width: 100%; padding: 7px 6px; text-align: left; white-space: nowrap; text-overflow: ellipsis; overflow: hidden; font: inherit; font-size: 12px; color: var(--muted, #656577); border: 0; border-radius: 4px; background: transparent; cursor: pointer; }
  .annotation-list button:hover, .annotation-list button.selected { background: var(--accent-soft, #eae5f7); color: var(--accent, #6850ac); }
  .original-note { display: flex; align-items: center; gap: 6px; margin: auto 6px 0; padding-top: 20px; color: var(--muted, #656577); font-size: 11px; }
  .canvas-panel { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .canvas-toolbar { display: flex; align-items: center; gap: 12px; min-height: 51px; padding: 7px 14px; flex-wrap: wrap; background: var(--surface, #fff); border-bottom: 1px solid var(--border, #dedfe7); }
  .history-actions { display: flex; align-items: center; gap: 3px; }
  .toolbar-divider { width: 1px; height: 20px; background: var(--border, #dedfe7); margin: 0 5px; }
  .context-help { font-size: 12px; color: var(--muted, #656577); flex: 1; }
  .clear-annotations, .restore-crop { font-size: 12px; white-space: nowrap; }
  .crop-confirm { display: flex; align-items: center; gap: 12px; min-height: 50px; padding: 8px 16px; background: var(--accent-soft, #eae5f7); color: var(--text, #242334); }
  .crop-confirm > span { margin-right: auto; font-variant-numeric: tabular-nums; }
  .editor-error { display: flex; align-items: center; gap: 16px; padding: 12px 16px; color: var(--danger, #a82a3e); background: var(--surface, #fff); border-bottom: 1px solid var(--border, #dedfe7); font-size: 13px; line-height: 1.45; }
  .editor-error span { flex: 1; overflow-wrap: anywhere; }
  .canvas-scroller { flex: 1; min-height: 0; overflow: auto; background: var(--bg, #f0f1f6); }
  .canvas-space { min-width: 100%; min-height: 100%; width: max-content; display: grid; place-items: center; padding: 32px; box-sizing: border-box; }
  .canvas-document { position: relative; width: max-content; box-shadow: 0 4px 20px #15182924; }
  .canvas-host { touch-action: none; }
  .canvas-host:focus { outline: none; }
  .canvas-host:focus-visible { outline: 2px solid var(--accent, #6850ac); outline-offset: 5px; }
  .canvas-host :global(canvas) { display: block; }
  .text-edit-wrapper { position: absolute; transform-origin: top left; z-index: 5; min-width: 80px; }
  .text-edit-wrapper textarea { display: block; box-sizing: border-box; width: 100%; min-height: 1.5em; padding: 0; margin: 0; border: 1px solid #7560c7; outline: 2px solid #fff; border-radius: 0; background: #ffffffe6; color: inherit; font: inherit; font-size: inherit; line-height: 1.25; resize: vertical; }
  .text-edit-actions { display: flex; justify-content: flex-start; gap: 1px; width: max-content; margin-top: 5px; background: #302947; box-shadow: 0 4px 12px #0003; border-radius: 5px; padding: 3px; }
  .text-edit-actions button { display: inline-flex; align-items: center; justify-content: center; gap: 5px; min-height: 29px; padding: 3px 8px; background: transparent; color: #fff; border: 0; border-radius: 3px; font: 12px "Segoe UI", sans-serif; cursor: pointer; }
  .text-edit-actions button:hover { background: #544667; }
  .text-edit-actions button:focus-visible { outline: 2px solid #ddcfff; }
  .empty-capture { display: flex; flex-direction: column; justify-content: center; align-items: center; gap: 12px; min-height: 100%; padding: 40px; box-sizing: border-box; text-align: center; }
  .empty-capture > :global(svg) { color: var(--accent, #6850ac); margin-bottom: 8px; }
  .empty-capture h2 { font-size: 20px; font-weight: 600; margin: 0; }
  .empty-capture p { font-size: 13px; color: var(--muted, #656577); max-width: 42ch; line-height: 1.6; margin: 0 0 8px; }
  .canvas-footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; min-height: 44px; padding: 6px 14px; flex: 0 0 auto; background: var(--surface, #fff); border-top: 1px solid var(--border, #dedfe7); }
  .capture-dimensions { display: flex; align-items: center; gap: 9px; font-size: 12px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .capture-dimensions span { color: var(--muted, #656577); font-size: 11px; }
  .draft-status { flex: 1; text-align: center; color: var(--muted, #656577); font-size: 11px; }
  .zoom-actions { display: flex; align-items: center; gap: 2px; }
  .zoom-value { min-width: 51px; padding: 5px; border: 0; border-radius: 5px; background: transparent; color: var(--text, #242334); font: inherit; font-size: 12px; font-variant-numeric: tabular-nums; cursor: pointer; }
  .zoom-value:hover { background: var(--accent-soft, #eae5f7); }
  .zoom-value:focus-visible { outline: 2px solid var(--accent, #6850ac); }
  @media (max-width: 1050px) { .context-help { display: none; } .clear-annotations { margin-left: auto; } .editor-title p { display: none; } .editor-header { gap: 14px; padding: 15px 16px; } }
  @media (max-width: 800px) { .editor-body { grid-template-columns: 142px minmax(0,1fr); } .tool-button { padding: 8px 7px; gap: 7px; } .tool-button kbd { display: none; } .color-list { grid-template-columns: repeat(3, 1fr); gap: 8px; } .color-swatch { width: 26px; height: 26px; } .save-button span { display: none; } .draft-status { display: none; } .editor-title > :global(svg) { display: none; } .canvas-toolbar { gap: 6px; padding: 7px 8px; } .canvas-footer { gap: 8px; padding: 6px 8px; } }
  @media (prefers-reduced-motion: reduce) { * { scroll-behavior: auto; } }
</style>
