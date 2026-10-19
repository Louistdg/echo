<script lang="ts">
  // Fenêtre flottante de dictée (label « quick »).
  // La capture audio n'est pas encore implémentée : le bouton est désactivé
  // et l'affiche honnêtement — aucun faux succès.
  let etat = $state("Prêt");
</script>

<main>
  <p class="compteur" aria-live="polite">{etat}</p>
  <button
    disabled
    title="Capture audio : à venir en v1 (module cpal)"
    onclick={() => (etat = "Capture non implémentée")}
  >
    Micro (à venir)
  </button>
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
    padding: 1.25rem 1.5rem;
    height: 100vh;
    box-sizing: border-box;
  }

  .compteur {
    margin: 0;
    font-size: 1rem;
  }

  button {
    font: inherit;
    padding: 0.6rem 1.1rem;
    border-radius: 999px;
    border: 1px solid #55555f;
    background: #2a2a33;
    color: #c9c9d4;
    cursor: not-allowed;
  }
</style>
