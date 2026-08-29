<script lang="ts">
  // Renders a Server settings page only once its config has loaded, and says
  // plainly what happened when it has not.
  //
  // WHY THIS EXISTS. Every page under Settings › Server was `{#if server.cfg}`
  // and nothing else, so a failed `/api/config/global` produced a completely
  // blank page — no error, no spinner, no reason. On 2026-08-29 a VPN came up
  // and took the 192.168.1.0/24 route with it; the open browser could no
  // longer reach the API, five settings pages went empty at once, and the
  // honest reading from the other side of the screen was **"all settings have
  // vanished"**. Nothing was wrong with the data at all.
  //
  // A settings screen that renders nothing on a failed fetch is
  // indistinguishable from one whose configuration has been destroyed. That is
  // the worst thing a settings screen can be mistaken for, so it is worth a
  // component: the failure now names itself, says the config is untouched, and
  // offers the retry.
  import type { Snippet } from 'svelte';
  import { server, loadServerConfig } from './serverconfig.svelte';

  let { children }: { children: Snippet } = $props();

  let retrying = $state(false);
  async function retry() {
    retrying = true;
    server.error = '';
    await loadServerConfig(true);
    retrying = false;
  }
</script>

{#if server.cfg}
  {@render children()}
{:else if server.error}
  <div class="card">
    <h2>Can't reach the server</h2>
    <p class="err">{server.error}</p>
    <p class="hint">
      This is the settings <em>page</em> failing to load, not your
      configuration. Nothing has been changed or lost — sources, nodes and
      destinations are held in <code>config/dxca.toml</code> on the server and
      are untouched by a failed read.
    </p>
    <p class="hint">
      Usually the browser cannot reach DXCA: a VPN that has taken the route to
      it, the machine asleep, or the service restarting.
    </p>
    <div class="actions">
      <button class="primary" onclick={retry} disabled={retrying}>
        {retrying ? 'Retrying…' : 'Try again'}
      </button>
    </div>
  </div>
{:else}
  <div class="card"><p class="hint">Loading…</p></div>
{/if}

<style>
  p {
    margin: 0.75rem 0 0;
  }
</style>
