<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import ImageIcon from 'lucide-svelte/icons/image';
  let { path, name }: { path: string; name: string } = $props();
  let host: HTMLDivElement;
  let source = $state('');
  let failed = $state(false);
  onMount(() => {
    let alive = true;
    let requested = false;
    const load = async () => {
      if (requested) return;
      requested = true;
      try { const result = await invoke<string>('asset_preview', { path }); if (alive) source = result; }
      catch { if (alive) failed = true; }
    };
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) { observer.disconnect(); void load(); }
    }, { rootMargin: '100px' });
    observer.observe(host);
    return () => { alive = false; observer.disconnect(); };
  });
</script>
<div class="asset-preview" bind:this={host}>
  {#if source}<img src={source} alt={name} draggable="false" />
  {:else}<span class="preview-message"><ImageIcon size={25} strokeWidth={1.5} />{failed ? 'Aperçu indisponible' : 'Chargement de l’image…'}</span>{/if}
</div>
<style>
  .asset-preview { height: 172px; display: grid; place-items: center; background: var(--sidebar); overflow: hidden; }
  img { width: 100%; height: 100%; object-fit: contain; }
  .preview-message { display: grid; gap: 10px; justify-items: center; color: var(--muted); font-size: 12px; }
</style>
