<script lang="ts">
  // "A newer DXCA is out", across the top of every screen — for admins only.
  //
  // Only an admin can upgrade the server, and a notice the reader cannot act
  // on is noise above the one screen they watch. The server decides what is
  // newer and remembers what was skipped (`update.rs`); this draws the answer
  // `/api/status` already carries, so it costs no request of its own.
  //
  // There is no Download button and no "install now": DXCA runs as a service
  // on a Pi, in Docker, on Windows or macOS, each updated its own way, so the
  // link goes to the release page — notes, the Windows zip, and the README's
  // Updating section one click on.
  import { api } from './api';
  import { status, refreshStatus } from './status.svelte';

  let u = $derived(status()?.update);
  let busy = $state(false);
  let error = $state('');

  async function skip() {
    busy = true;
    error = '';
    const r = await api('POST', '/api/update/skip', { tag: u.tag });
    busy = false;
    if (r.status === 200) await refreshStatus();
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }
</script>

{#if u && !u.skipped}
  <div class="update-banner" role="status">
    <span>
      <b>DXCA v{u.version}</b> is available (you have v{u.current}) —
      <a href={u.url} target="_blank" rel="noopener noreferrer">release notes &amp; download ↗</a>
    </span>
    <button
      onclick={skip}
      disabled={busy}
      title="Hide this notice until the release after v{u.version}. Settings › Server › Reference data can bring it back."
      >Skip this version</button
    >
    {#if error}<span class="err">{error}</span>{/if}
  </div>
{/if}

<style>
  /* A band under the header rule, not a card: it is about the whole app, and
     it should read as news without shouting over the spot feed below it. */
  .update-banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    padding: 0.5rem 1.25rem;
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--accent) 10%, Canvas);
    font-size: 0.85rem;
  }

  a {
    color: var(--accent);
    text-decoration: none;
    white-space: nowrap;
  }

  a:hover {
    text-decoration: underline;
  }

  .err {
    color: var(--err);
  }
</style>
