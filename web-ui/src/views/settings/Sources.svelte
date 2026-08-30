<script lang="ts">
  // Settings › Server › Sources — everywhere a spot arrives from.
  //
  // Two kinds, one question, mirroring Destinations on the other side of the
  // pipeline: decoders on this machine's LAN send UDP, and cluster nodes are
  // dialled out to over telnet. They were two rail entries; they are one
  // subject.
  //
  // A thin wrapper on purpose. Each tab keeps its own `ConfigGate`, its own
  // card and its own save button — the two write different slices, and the
  // components are unchanged by being tabbed.
  import UdpSources from './UdpSources.svelte';
  import ClusterNodes from './ClusterNodes.svelte';

  // Remembered per browser, as on Stats and Destinations: a segmented control
  // that has to be re-found on every visit gets missed, and someone who came
  // for their node list would think it had gone.
  const SEG_KEY = 'dxca.srcseg';
  type Seg = 'udp' | 'nodes';
  function restoreSeg(): Seg {
    try {
      return localStorage.getItem(SEG_KEY) === 'nodes' ? 'nodes' : 'udp';
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
</script>

<div class="segmented" role="tablist" aria-label="Which sources">
  <button role="tab" aria-selected={seg === 'udp'} class:active={seg === 'udp'}
    onclick={() => pick('udp')}>UDP</button>
  <button role="tab" aria-selected={seg === 'nodes'} class:active={seg === 'nodes'}
    onclick={() => pick('nodes')}>Cluster nodes</button>
</div>

{#if seg === 'udp'}
  <UdpSources />
{:else}
  <ClusterNodes />
{/if}

<style>
  /* The control sits above the cards, so it needs the gap they would
     otherwise have given it. */
  .segmented {
    margin-bottom: 1rem;
  }
</style>
