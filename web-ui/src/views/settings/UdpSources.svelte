<script lang="ts">
  // Settings › Server › UDP sources — the decoder feeds DXCA listens on.
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';
  import ApplySave from '../../lib/ApplySave.svelte';
  import {
    server, loadServerConfig, drop, SOURCE_NAME_MAX,
  } from '../../lib/serverconfig.svelte';

  onMount(loadServerConfig);

  const add = () =>
    (server.cfg.udp_sources = [
      ...server.cfg.udp_sources,
      { name: '', port: 2336, enabled: true },
    ]);
</script>

{#if server.cfg}
  <div class="card">
    <h2>
      UDP sources
      <HelpTip label="UDP sources">
        <span class="para">
          One listener per decoder — WSJT-X, JTDX, MSHV, RUMlog. The
          <b>name</b> is what the spot is filed under: it is the label on the
          Sources chips, the Source column in the feed, and what a broadcast
          destination matches on.
        </span>
        <span class="para">
          Keep names to <b>{SOURCE_NAME_MAX} characters</b>. The feed's Source
          column is a fixed width measured against exactly that, so a longer
          name is the one thing an operator can do that makes their own table
          clip.
        </span>
      </HelpTip>
    </h2>
    <div class="editor-scroll">
      <table class="editor">
        <thead><tr><th>Name</th><th>Port</th><th>On</th><th></th></tr></thead>
        <tbody>
          {#each server.cfg.udp_sources as s, i}
            <tr>
              <td>
                <input bind:value={s.name} maxlength={SOURCE_NAME_MAX} />
                {#if (s.name ?? '').length >= SOURCE_NAME_MAX}
                  <span class="cap" title="The feed's Source column is sized to {SOURCE_NAME_MAX} characters">max</span>
                {/if}
              </td>
              <td><input type="number" bind:value={s.port} class="port" /></td>
              <td><input type="checkbox" bind:checked={s.enabled} /></td>
              <td>
                <button class="drop" title="Remove"
                  onclick={() => (server.cfg.udp_sources = drop(server.cfg.udp_sources, i))}>✕</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="actions"><button onclick={add}>+ Add source</button></div>
    <ApplySave />
  </div>
{/if}
