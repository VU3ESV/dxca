<script lang="ts">
  // M0 stub shell: proves the embedded-UI + API wiring end to end.
  // M5 replaces this with the real dashboard (spots table, filters,
  // status pills) per docs/PLAN.md §8.
  type Status = { name: string; version: string; milestone: string };

  let status = $state<Status | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    fetch('/api/status')
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`HTTP ${r.status}`))))
      .then((s: Status) => (status = s))
      .catch((e: Error) => (error = e.message));
  });
</script>

<header>
  <h1>DXCA</h1>
  <span class="tag">FT8/FT4 + DX-cluster spot aggregator</span>
</header>

<main>
  {#if status}
    <p>
      Server <code>v{status.version}</code> is up — <strong>{status.milestone}</strong>.
    </p>
    <p class="dim">
      The spots dashboard lands in M5. Until then this page just proves the
      Rust server, embedded assets, and API are wired.
    </p>
  {:else if error}
    <p class="err">API unreachable: {error}</p>
  {:else}
    <p class="dim">Contacting server…</p>
  {/if}
</main>

<style>
  header {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding: 12px 20px;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border);
  }
  h1 {
    margin: 0;
    font-size: 18px;
    color: var(--accent);
  }
  .tag {
    color: var(--fg-dim);
    font-size: 12px;
  }
  main {
    padding: 20px;
    max-width: 720px;
  }
  .dim {
    color: var(--fg-dim);
  }
  .err {
    color: var(--red);
  }
</style>
