<script lang="ts">
  // Settings › Server › Spot outputs — every way a spot leaves DXCA.
  //
  // Renamed from "Broadcast destinations": nothing here broadcasts, they are
  // unicast UDP sends and MQTT publishes, and "outputs" pairs with the UDP
  // sources and cluster nodes above, which are where spots come IN.
  //
  // UDP and MQTT are one page because they answer one question: where do spots
  // go out. They keep SEPARATE save buttons, which looks like an inconsistency
  // and is not — the UDP rows live in config/dxca.toml, the MQTT rows carry a
  // broker password and live in the 0600 database. One button would have to
  // write two stores and half-succeed.
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';
  import ApplySave from '../../lib/ApplySave.svelte';
  import ConfigGate from '../../lib/ConfigGate.svelte';
  import Mqtt from './Mqtt.svelte';
  import { server, loadServerConfig, drop } from '../../lib/serverconfig.svelte';

  onMount(loadServerConfig);

  const add = () =>
    (server.cfg.broadcast_destinations = [
      ...server.cfg.broadcast_destinations,
      {
        name: '', ip: '127.0.0.1', port: 2237, format: 'passthrough',
        sources: [], unfiltered: false, enabled: true,
      },
    ]);
</script>

<ConfigGate>
  <div class="card">
    <h2>
      Spot outputs
      <HelpTip label="Spot outputs">
        <span class="para">
          Each row is one UDP feed out. <b>cluster</b> sends the plain
          DX-cluster line, <b>wsjtx</b> the WSJT-X decode datagram, and
          <b>passthrough</b> forwards the decoder's own datagram untouched —
          before any parsing, which is why a blacklisted call can still reach a
          logger that way.
        </span>
        <span class="para">
          <b>Sources</b> empty means every source. <b>Unf</b> bypasses the
          dedupe window, so the destination sees every copy of a spot rather
          than the first.
        </span>
      </HelpTip>
    </h2>
    <div class="editor-scroll">
      <table class="editor">
        <thead>
          <tr>
            <th>Name</th><th>IP</th><th>Port</th><th>Format</th>
            <th>Sources (CSV, empty = all)</th><th>Unf</th><th>On</th><th></th>
          </tr>
        </thead>
        <tbody>
          {#each server.cfg.broadcast_destinations as d, i}
            <tr>
              <td><input bind:value={d.name} /></td>
              <td><input bind:value={d.ip} class="host" /></td>
              <td><input type="number" bind:value={d.port} class="port" /></td>
              <td>
                <select bind:value={d.format}>
                  <option value="cluster">cluster</option>
                  <option value="wsjtx">wsjtx</option>
                  <option value="passthrough">passthrough</option>
                </select>
              </td>
              <td>
                <input
                  class="csv"
                  value={d.sources.join(', ')}
                  onchange={(e: any) =>
                    (d.sources = e.target.value.split(',').map((x: string) => x.trim()).filter(Boolean))}
                />
              </td>
              <td><input type="checkbox" bind:checked={d.unfiltered} title="Unfiltered: bypass dedupe" /></td>
              <td><input type="checkbox" bind:checked={d.enabled} /></td>
              <td>
                <button class="drop" title="Remove"
                  onclick={() =>
                    (server.cfg.broadcast_destinations = drop(server.cfg.broadcast_destinations, i))}>✕</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="actions"><button onclick={add}>+ Add destination</button></div>
    <ApplySave />
  </div>

  <Mqtt />
</ConfigGate>
