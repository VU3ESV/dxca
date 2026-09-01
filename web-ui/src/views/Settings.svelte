<script lang="ts">
  // The Settings view — everything that is setup, behind the gear.
  //
  // Shaped after Meridian's: the gear swaps the whole view rather than adding a
  // tab, the header's tab strip is replaced by the word "Settings", and a
  // grouped left rail navigates. Matching it is deliberate — the two apps run
  // in the same shack, and one muscle memory is worth more than any local
  // improvement.
  //
  // The grouping is by OWNERSHIP, which is the question an operator actually
  // has: is this mine, is this the server's, or is this about who may log in.
  import Users from './Users.svelte';
  import ClubLogAccount from './settings/ClubLogAccount.svelte';
  import LotwAccount from './settings/LotwAccount.svelte';
  import AwardSettings from './settings/AwardSettings.svelte';
  import Station from './settings/Station.svelte';
  import Telegram from './settings/Telegram.svelte';
  import ReferenceData from './settings/ReferenceData.svelte';
  import Sources from './settings/Sources.svelte';
  import Destinations from './settings/Destinations.svelte';

  let { isAdmin }: { isAdmin: boolean } = $props();

  /// `find` is what the search box matches on, beyond the label itself. It is
  /// the words an operator would actually type when they know WHAT they want
  /// to change but not which page it lives on — "token", "grid square",
  /// "skimmer", "broker". Without it a search box only finds pages you could
  /// already see, which is no help at all.
  type Item = { key: string; label: string; find: string };
  const GROUPS: { head: string; admin: boolean; items: Item[] }[] = [
    {
      head: 'My station',
      admin: false,
      items: [
        { key: 'clublog', label: 'ClubLog',
          find: 'clublog credentials email app password callsign log download refresh auto-refresh api matrix' },
        { key: 'lotw', label: 'LoTW',
          find: 'lotw logbook of the world arrl login username password qsl report confirmed confirmation '
              + 'state grid island award was vucc iota credentials' },
        // The whole alert ladder lives here — DXCC's classic eight and the
        // chaseable awards — so every level word finds this page.
        { key: 'awards', label: 'Awards',
          find: 'award awards chasing chase iota was vucc grid grids square state states island islands '
              + 'alert level levels new dxcc band mode slot unconfirmed confirmed flag classifier highlight ladder' },
        { key: 'station', label: 'Locator & grey line',
          find: 'locator maidenhead grid square qth sun sunrise sunset greyline grey line band mask dawn dusk' },
        { key: 'telegram', label: 'Telegram',
          find: 'telegram bot token chat id botfather cooldown notify ping push message health feed quiet node down' },
      ],
    },
    {
      head: 'Server',
      admin: true,
      items: [
        { key: 'reference', label: 'Reference data',
          find: 'cty cty.xml dxcc prefix entity lotw api key blacklist blocked block ban version milestone' },
        // Everywhere a spot arrives from, in one place: the decoders this
        // machine listens for, and the cluster nodes it dials. Mirrors
        // Destinations on the other side of the pipeline.
        { key: 'sources', label: 'Sources',
          find: 'source sources udp decoder wsjt wsjtx jtdx mshv rumlog port listener '
              + 'cluster node nodes telnet dx spider upstream login host skimmer feed ingest' },
        // Everything a spot can leave by, in one place: UDP, MQTT and the
        // FlexRadio panadapter, which used to be its own entry under My
        // station. The Flex keywords stay in this `find` string so searching
        // "panadapter" still lands here.
        { key: 'dests', label: 'Destinations',
          find: 'destination destinations spot output outputs broadcast udp out wsjtx passthrough logger '
              + 'mqtt broker topic publish unfiltered flex flexradio smartsdr panadapter radio 4992 colour color lifetime' },
      ],
    },
    {
      head: 'Access',
      admin: true,
      items: [
        { key: 'users', label: 'Users',
          find: 'user account admin password role login delete add operator' },
      ],
    },
  ];

  let tab = $state('clublog');
  let query = $state('');

  let visible = $derived(
    GROUPS.filter((g) => !g.admin || isAdmin)
      .map((g) => {
        const q = query.trim().toLowerCase();
        if (!q) return g;
        // Every word must appear somewhere — "grey line" and "line grey" find
        // the same page, and a partial word still matches as you type.
        const words = q.split(/\s+/);
        const hit = (i: Item) => {
          const hay = `${i.label} ${i.find} ${g.head}`.toLowerCase();
          return words.every((w) => hay.includes(w));
        };
        return { ...g, items: g.items.filter(hit) };
      })
      .filter((g) => g.items.length),
  );

  let hits = $derived(visible.flatMap((g) => g.items));

  // A demoted admin must not be left staring at a page they can no longer
  // load. Cheap to guard, and the alternative is a card of 403s. Guarded
  // against the FULL list, not the searched one — filtering the rail must
  // never navigate you away from the page you are reading.
  $effect(() => {
    const reachable = GROUPS.filter((g) => !g.admin || isAdmin)
      .flatMap((g) => g.items.map((i) => i.key));
    if (!reachable.includes(tab)) tab = 'clublog';
  });

  /// Enter on a search that has narrowed to one page opens it — the whole
  /// point of typing "token" is not to then have to aim at the result.
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') query = '';
    else if (e.key === 'Enter' && hits.length) tab = hits[0].key;
  }
</script>

<div class="rail-shell">
  <nav class="rail" aria-label="Settings">
    <input
      class="rail-search"
      type="search"
      placeholder="Find a setting"
      aria-label="Find a setting"
      bind:value={query}
      onkeydown={onKey}
    />
    {#each visible as g (g.head)}
      <div class="rail-group">
        <div class="rail-head">{g.head}</div>
        {#each g.items as i (i.key)}
          <button class="rail-item" class:active={tab === i.key} onclick={() => (tab = i.key)}>
            {i.label}
          </button>
        {/each}
      </div>
    {:else}
      <p class="hint norail">Nothing matches “{query}”.</p>
    {/each}
  </nav>

  <div class="page narrow settings-content">
    {#if tab === 'clublog'}
      <ClubLogAccount />
    {:else if tab === 'lotw'}
      <LotwAccount />
    {:else if tab === 'awards'}
      <AwardSettings />
    {:else if tab === 'station'}
      <Station />
    {:else if tab === 'telegram'}
      <Telegram />
    {:else if tab === 'reference'}
      <ReferenceData />
    {:else if tab === 'sources'}
      <Sources />
    {:else if tab === 'dests'}
      <Destinations />
    {:else if tab === 'users'}
      <Users />
    {/if}
  </div>
</div>

<style>
  /* Sized and spaced like a rail item so it reads as part of the list rather
     than a control bolted above it. */
  .rail-search {
    font: inherit;
    font-size: 0.82rem;
    padding: 0.25rem 0.5rem;
    width: 100%;
    min-width: 0;
    margin-bottom: 0.25rem;
  }

  .norail {
    padding: 0 0.5rem;
    line-height: 1.4;
  }

  /* Every settings page is one or two cards in a readable column — wider than
     `.narrow` only where a list editor needs it, and those scroll inside their
     own card rather than stretching the page. */
  .settings-content {
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    max-width: 60rem;
  }

  /* Narrow windows: the rail goes on top as a scrolling strip rather than
     taking a column that leaves nothing for the content. */
  @media (max-width: 48rem) {
    .rail-shell {
      grid-template-columns: minmax(0, 1fr);
    }

    .rail {
      position: static;
      flex-direction: row;
      flex-wrap: wrap;
      gap: 0.5rem 1rem;
      max-height: none;
      border-right: none;
      border-bottom: 1px solid var(--border);
      padding: 0.75rem 1.25rem;
    }

    .rail-group {
      flex-direction: row;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.25rem;
    }

    .rail-head {
      padding: 0 0.35rem 0 0;
    }
  }
</style>
