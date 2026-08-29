<script lang="ts">
  // Server health, in the app header, on every screen.
  //
  // This replaces the four status boxes that used to sit between the station
  // card and the filters on Spots — Decoders, Cluster nodes, Feeds out,
  // Reference. Together they cost three rows and 220px of the one screen where
  // height is the scarce thing. As a pill they cost nothing, and they are now
  // visible from Alerts and Stats too, which they never were.
  //
  // The pill states the one number worth watching — how many nodes are proven —
  // and the `?` carries everything the four boxes used to say.
  import { status, nodeHealth } from './status.svelte';
  import { ago } from './api';
  import HelpTip from './HelpTip.svelte';

  let s = $derived(status());
  let health = $derived(nodeHealth());
  /// A decoder is a spot source that is NOT also a cluster node: every node
  /// feeds `spots_per_source` too, so counting both lists double-counts.
  let decoders = $derived(
    Object.keys(s?.spots_per_source ?? {}).filter(
      (n) => !(n in (s?.cluster_nodes ?? {})),
    ),
  );
  /// Amber the moment anything configured is not proven — the pill's whole job
  /// is to be noticed when a feed has gone quiet.
  let dot = $derived(
    !s || health.total === 0
      ? 'warn'
      : health.proven === health.total
        ? 'on'
        : health.proven > 0
          ? 'warn'
          : 'err',
  );
</script>

{#if s}
  <span class="pill health">
    <span class="status-dot {dot}"></span>
    <!-- One flex child, not three: `.pill` gaps its children by 0.4rem, which
         fell between the bold figure and the slash and read as "7 /10". -->
    <span class="hc"><b>{health.proven}</b>/{health.total} nodes</span>
    <HelpTip label="Server health">
      <span class="para">
        <b>{health.proven} of {health.total}</b> cluster nodes proven —
        connected and having passed a spot through. Decoders feeding in:
        {#if decoders.length}<b>{decoders.join(', ')}</b>{:else}none{/if}.
      </span>
      <span class="para">
        Out: <b>{s.telnet_clients}</b> telnet client{s.telnet_clients === 1
          ? ''
          : 's'}, <b>{s.udp_sent}</b> UDP datagram{s.udp_sent === 1 ? '' : 's'}
        sent{#if s.udp_failed}, <b>{s.udp_failed}</b> failed{/if}. Reference:
        <b>{s.cty_entities}</b> DXCC entities, <b>{s.lotw_users}</b> LoTW users.
      </span>
      {#if health.total}
        <span class="para nodes">
          {#each Object.entries(s.cluster_nodes ?? {}) as [name, n] (name)}
            <span class="node">
              <span class="status-dot {n.proven ? 'on' : n.connected ? 'warn' : 'err'}"></span>
              <span class="nm">{name}</span>
              <span class="ct">{n.spot_count}</span>
              <span class="ag">{ago(n.last_spot_unix)}</span>
            </span>
          {/each}
        </span>
      {/if}
      <span class="para">Per-node detail and the editors live in <b>Settings › Server</b>.</span>
    </HelpTip>
  </span>
{/if}

<style>
  /* Reads as a fact about the session, like the callsign chip beside it — not
     as something to press. */
  .health {
    font-size: 0.78rem;
    white-space: nowrap;
  }

  .hc {
    font-variant-numeric: tabular-nums;
  }

  /* A compact node roster inside the popover: the old Cluster nodes box, in
     the place the box used to occupy on the page. */
  .nodes {
    display: grid;
    gap: 0.15rem;
  }

  .node {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.74rem;
  }

  .node .nm {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .node .ct,
  .node .ag {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
</style>
