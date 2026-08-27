<script lang="ts">
  // App shell: session bootstrap → setup / login / tabbed main UI.
  import { api } from './lib/api';
  import { onMount } from 'svelte';
  import ThemeSwitcher from './lib/ThemeSwitcher.svelte';
  import Auth from './views/Auth.svelte';
  import Dashboard from './views/Dashboard.svelte';
  import ClubLog from './views/ClubLog.svelte';
  import Alerts from './views/Alerts.svelte';
  import Users from './views/Users.svelte';
  import System from './views/System.svelte';

  type View = 'loading' | 'setup' | 'login' | 'main';
  let view = $state<View>('loading');
  let me = $state<any>(null);
  let tab = $state('spots');
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

  async function logout() {
    await api('POST', '/api/logout');
    me = null;
    tab = 'spots';
    view = 'login';
  }

  let tabs = $derived([
    ['spots', 'Spots'],
    ['clublog', 'My ClubLog'],
    ['alerts', 'My Alerts'],
    ...(me?.role === 'admin' ? [['users', 'Users']] : []),
    ['system', 'System'],
  ] as [string, string][]);
</script>

{#if view === 'loading'}
  <p class="hint" style="padding: 1.5rem 1.25rem">Contacting server…</p>
{:else if view === 'setup' || view === 'login'}
  <Auth mode={view} onDone={bootstrap} />
{:else}
  <header>
    <h1>DXCA{#if version}&nbsp;<span class="app-version">v{version}</span>{/if}</h1>
    <nav class="tabs">
      {#each tabs as [id, label]}
        <button class:active={tab === id} onclick={() => (tab = id)}>{label}</button>
      {/each}
    </nav>
    <div class="header-right">
      <span class="profile-display">
        <span class="call">{me.callsign}</span>
        {#if me.role === 'admin'}admin{/if}
      </span>
      <ThemeSwitcher />
      <button onclick={logout}>Log out</button>
    </div>
  </header>
  {#if tab === 'spots'}
    <Dashboard />
  {:else if tab === 'clublog'}
    <ClubLog />
  {:else if tab === 'alerts'}
    <Alerts />
  {:else if tab === 'users'}
    <Users />
  {:else if tab === 'system'}
    <System isAdmin={me.role === 'admin'} />
  {/if}
{/if}
