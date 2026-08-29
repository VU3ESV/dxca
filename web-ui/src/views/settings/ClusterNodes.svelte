<script lang="ts">
  // Settings › Server › Cluster nodes — the upstream telnet feeds, and how
  // each is doing.
  //
  // Live state and config sat in two separate cards on the old System tab,
  // several screens apart. They are one subject: the answer to "why is DB0SUE
  // quiet" is usually in the row right below the reading.
  import { onMount } from 'svelte';
  import { api, ago } from '../../lib/api';
  import { status, refreshStatus } from '../../lib/status.svelte';
  import HelpTip from '../../lib/HelpTip.svelte';
  import ApplySave from '../../lib/ApplySave.svelte';
  import {
    server, loadServerConfig, drop, SOURCE_NAME_MAX,
  } from '../../lib/serverconfig.svelte';

  let s = $derived(status());

  onMount(() => {
    loadServerConfig();
    refreshStatus();
    const t = setInterval(refreshStatus, 5000);
    return () => clearInterval(t);
  });

  const add = () =>
    (server.cfg.cluster_nodes = [
      ...server.cfg.cluster_nodes,
      { name: '', host: '', port: 7300, login_call: '', password: '', enabled: true },
    ]);
</script>

<div class="card">
  <h2>
    Cluster nodes — live
    <HelpTip label="Node state">
      <b>Proven</b> means connected AND having passed a spot through — the only
      state worth trusting, because a node can hold a socket open for hours
      without ever sending anything. Amber is connected but unproven; red is
      not connected.
    </HelpTip>
  </h2>
  {#if s}
    <table>
      <thead><tr><th>Node</th><th>State</th><th>Spots</th><th>Last spot</th><th>Attempts</th></tr></thead>
      <tbody>
        {#each Object.entries(s.cluster_nodes ?? {}) as [name, n] (name)}
          <tr>
            <td>{name}</td>
            <td>
              <span class="status-dot {n.proven ? 'on' : n.connected ? 'warn' : 'err'}"></span>
              {n.state}
            </td>
            <td class="num">{n.spot_count}</td>
            <td class="num">{ago(n.last_spot_unix)}</td>
            <td class="num">{n.attempt}</td>
          </tr>
        {:else}
          <tr><td colspan="5" class="hint">None configured.</td></tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="hint">Loading…</p>
  {/if}
</div>

{#if server.cfg}
  <div class="card">
    <h2>
      Cluster nodes — config
      <HelpTip label="Cluster nodes">
        <span class="para">
          Upstream DX-cluster nodes DXCA logs into over telnet. Every node also
          counts as a spot <b>source</b>, so its name appears on the Sources
          chips and in the feed's Source column.
        </span>
        <span class="para">
          Which is why the name is capped at <b>{SOURCE_NAME_MAX}
          characters</b>: the feed's Source column is a fixed width measured
          against exactly that.
        </span>
      </HelpTip>
    </h2>
    <div class="editor-scroll">
      <table class="editor">
        <thead><tr><th>Name</th><th>Host</th><th>Port</th><th>Login</th><th>Password</th><th>On</th><th></th></tr></thead>
        <tbody>
          {#each server.cfg.cluster_nodes as n, i}
            <tr>
              <td>
                <input bind:value={n.name} maxlength={SOURCE_NAME_MAX} />
                {#if (n.name ?? '').length >= SOURCE_NAME_MAX}
                  <span class="cap" title="The feed's Source column is sized to {SOURCE_NAME_MAX} characters">max</span>
                {/if}
              </td>
              <td><input bind:value={n.host} class="host" /></td>
              <td><input type="number" bind:value={n.port} class="port" /></td>
              <td><input bind:value={n.login_call} class="call" /></td>
              <td><input type="password" bind:value={n.password} class="call" /></td>
              <td><input type="checkbox" bind:checked={n.enabled} /></td>
              <td>
                <button class="drop" title="Remove"
                  onclick={() => (server.cfg.cluster_nodes = drop(server.cfg.cluster_nodes, i))}>✕</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="actions"><button onclick={add}>+ Add node</button></div>
    <ApplySave />
  </div>
{/if}
