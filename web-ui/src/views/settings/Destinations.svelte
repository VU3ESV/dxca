<script lang="ts">
  // Settings › Server › Destinations — every way a spot leaves DXCA.
  //
  // Four kinds, one question: where do spots go out. UDP sends, MQTT
  // publishes, and the two radio displays — the FlexRadio panadapter, which
  // used to be its own entry under My station until it was pointed out that a
  // radio is a destination like any other, and the ExpertSDR3 panorama over
  // TCI beside it.
  //
  // Called "Destinations" rather than "Spot outputs" because "outputs" reads
  // oddly for a radio; and not "Broadcast destinations", the older name,
  // because nothing here broadcasts — they are unicast sends and publishes.
  //
  // Each tab keeps its OWN save button, which looks like an inconsistency and
  // is not: the UDP rows live in config/dxca.toml, the MQTT rows carry a
  // broker password and live in the 0600 database, and the Flex and TCI
  // settings are in the account's notify row. One button would have to write
  // three stores and could half-succeed.
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';
  import ApplySave from '../../lib/ApplySave.svelte';
  import ConfigGate from '../../lib/ConfigGate.svelte';
  import Mqtt from './Mqtt.svelte';
  import FlexRadio from './FlexRadio.svelte';
  import Tci from './Tci.svelte';
  import { server, loadServerConfig, drop } from '../../lib/serverconfig.svelte';

  onMount(loadServerConfig);

  // Which tab is showing, remembered per browser — the same reasoning as the
  // Stats segmented control: a control that has to be found again on every
  // visit gets missed, and someone who came for the panadapter settings would
  // conclude they had gone.
  const SEG_KEY = 'dxca.destseg';
  type Seg = 'udp' | 'mqtt' | 'flex' | 'tci';
  function restoreSeg(): Seg {
    try {
      const v = localStorage.getItem(SEG_KEY);
      return v === 'mqtt' || v === 'flex' || v === 'tci' ? v : 'udp';
    } catch {
      return 'udp';
    }
  }
  let seg = $state<Seg>(restoreSeg());
  function pick(v: Seg) {
    seg = v;
    try {
      localStorage.setItem(SEG_KEY, v);
    } catch {
      // Private mode or storage disabled: the tab still works this session.
    }
  }

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
  <div class="segmented" role="tablist" aria-label="Which destinations">
    <button role="tab" aria-selected={seg === 'udp'} class:active={seg === 'udp'}
      onclick={() => pick('udp')}>UDP</button>
    <button role="tab" aria-selected={seg === 'mqtt'} class:active={seg === 'mqtt'}
      onclick={() => pick('mqtt')}>MQTT</button>
    <button role="tab" aria-selected={seg === 'flex'} class:active={seg === 'flex'}
      onclick={() => pick('flex')}>FlexRadio</button>
    <button role="tab" aria-selected={seg === 'tci'} class:active={seg === 'tci'}
      onclick={() => pick('tci')}>TCI</button>
  </div>

  {#if seg === 'udp'}
  <div class="card">
    <h2>
      UDP destinations
      <HelpTip label="UDP destinations">
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
            <!-- Sources is LAST, after the two tickboxes and the delete
                 button: it is the widest column by far, and in the middle it
                 pushed Unf / On / ✕ off the right edge on a laptop, so the
                 controls you actually reach for needed a horizontal scroll.
                 The narrow, high-traffic controls come first; the free-text
                 field takes whatever width is left. -->
            <th>Name</th><th>IP</th><th>Port</th><th>Format</th>
            <th>Unf</th><th>On</th><th></th><th>Sources (CSV, empty = all)</th>
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
              <td><input type="checkbox" bind:checked={d.unfiltered} title="Unfiltered: bypass dedupe" /></td>
              <td><input type="checkbox" bind:checked={d.enabled} /></td>
              <td>
                <button class="drop" title="Remove"
                  onclick={() =>
                    (server.cfg.broadcast_destinations = drop(server.cfg.broadcast_destinations, i))}>✕</button>
              </td>
              <td>
                <input
                  class="csv"
                  value={d.sources.join(', ')}
                  onchange={(e: any) =>
                    (d.sources = e.target.value.split(',').map((x: string) => x.trim()).filter(Boolean))}
                />
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="actions"><button onclick={add}>+ Add destination</button></div>
    <ApplySave />
  </div>
  {:else if seg === 'mqtt'}
    <Mqtt />
  {:else if seg === 'flex'}
    <FlexRadio />
  {:else}
    <Tci />
  {/if}
</ConfigGate>
