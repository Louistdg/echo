<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let { children } = $props();

  // Chaque fenêtre Tauri charge la même SPA (fallback index.html) ;
  // on route la fenêtre « quick » vers sa page dédiée.
  onMount(async () => {
    try {
      const label = getCurrentWindow().label;
      if (label === "quick" && window.location.pathname !== "/quick") {
        await goto("/quick");
      }
    } catch {
      // Hors Tauri (navigateur) : ne rien faire.
    }
  });
</script>

{@render children()}
