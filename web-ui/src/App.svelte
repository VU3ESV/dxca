<script lang="ts">
  // App shell: session bootstrap → setup / login / the main UI.
  //
  // Three tabs and a gear, after the 2026-08-29 cleanup. Spots, Alerts and
  // Stats are the screens you WATCH; everything you SET UP moved behind the
  // gear into Settings, which is Meridian's arrangement. Users, Blacklist and
  // System stopped being tabs; My ClubLog split in two, its credentials into
  // Settings and its statistics into Stats.
  import { api } from './lib/api';
  import { onMount } from 'svelte';
  import ThemeSwitcher from './lib/ThemeSwitcher.svelte';
  import StatusPill from './lib/StatusPill.svelte';
  import { refreshStatus } from './lib/status.svelte';
  import Auth from './views/Auth.svelte';
  import Dashboard from './views/Dashboard.svelte';
  import Alerts from './views/Alerts.svelte';
  import Stats from './views/Stats.svelte';
  import Settings from './views/Settings.svelte';

  type View = 'loading' | 'setup' | 'login' | 'main';
  let view = $state<View>('loading');
  let me = $state<any>(null);
  let tab = $state('spots');
  /// Settings is a MODE you enter and leave, not a fourth tab: it swaps the
  /// whole view and gives the tab strip's slot to its own name.
  let settings = $state(false);
  // Server version beside the wordmark. Read off the bootstrap status call
  // that already runs — the header costs no extra request for it.
  let version = $state('');

  async function bootstrap() {
    const status = await api('GET', '/api/status');
    if (status.json?.version) version = status.json.version;
    if (status.json?.setup_required) {
      view = 'setup';
      return;
    }
    const who = await api('GET', '/api/me');
    if (who.status === 200) {
      me = who.json;
      view = 'main';
    } else {
      view = 'login';
    }
  }
  onMount(bootstrap);

  // The header's health pill is on every screen, so its data cannot belong to
  // Spots. Polled slowly here; while Spots is open its SSE stream pushes
  // frames into the same store, so the pill is as live as the feed.
  $effect(() => {
    if (view !== 'main') return;
    refreshStatus();
    const t = setInterval(refreshStatus, 15000);
    return () => clearInterval(t);
  });

  async function logout() {
    await api('POST', '/api/logout');
    me = null;
    tab = 'spots';
    settings = false;
    view = 'login';
  }

  const TABS: [string, string][] = [
    ['spots', 'Spots'],
    ['alerts', 'Alerts'],
    ['stats', 'Stats'],
  ];
</script>

{#if view === 'loading'}
  <p class="hint" style="padding: 1.5rem 1.25rem">Contacting server…</p>
{:else if view === 'setup' || view === 'login'}
  <Auth mode={view} onDone={bootstrap} />
{:else}
  <header>
    <h1>DXCA{#if version}&nbsp;<span class="app-version">v{version}</span>{/if}</h1>
    {#if settings}
      <!-- Settings has no tab strip, so the section name takes that slot and
           shares its divider rule: the header always names what is open. -->
      <span class="section-label">Settings</span>
    {:else}
      <nav class="tabs">
        {#each TABS as [id, label] (id)}
          <button class:active={tab === id} onclick={() => (tab = id)}>{label}</button>
        {/each}
      </nav>
    {/if}
    <div class="header-right">
      <StatusPill />
      <span class="profile-display">
        <span class="call">{me.callsign}</span>
        {#if me.role === 'admin'}admin{/if}
      </span>
      <ThemeSwitcher />
      <button
        class="gear"
        class:active={settings}
        title={settings ? 'Close settings' : 'Settings'}
        aria-label="Settings"
        aria-pressed={settings}
        onclick={() => (settings = !settings)}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </button>
      <button onclick={logout}>Log out</button>
    </div>
  </header>

  {#if settings}
    <Settings isAdmin={me.role === 'admin'} />
  {:else if tab === 'spots'}
    <Dashboard />
  {:else if tab === 'alerts'}
    <Alerts />
  {:else if tab === 'stats'}
    <Stats />
  {/if}
{/if}
