<script lang="ts">
  // The Apply & save button the four Server pages share.
  //
  // One endpoint owns sources, nodes, destinations and the API key, so one
  // button saves them — and it has to appear on all four pages, because an
  // operator who edited a node and then clicked through to Destinations must
  // not have to remember which page the Save was on.
  import { server, saveServerConfig } from './serverconfig.svelte';
  import HelpTip from './HelpTip.svelte';
</script>

<div class="actions">
  <button class="primary" onclick={saveServerConfig} disabled={server.busy}>Apply &amp; save</button>
  <HelpTip label="Apply &amp; save">
    Applies live — listeners rebind, nodes redial, destinations re-point — and
    rewrites <code>config/dxca.toml</code>. One button for every page under
    <b>Server</b>, because the server stores them as one document.
  </HelpTip>
</div>
{#if server.message}<p class="ok">{server.message}</p>{/if}
{#if server.error}<p class="err">{server.error}</p>{/if}

<style>
  p {
    margin: 0.75rem 0 0;
  }
</style>
