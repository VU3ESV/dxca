<script lang="ts">
  // App shell: session bootstrap → setup / login / tabbed main UI.
  import { api } from './lib/api';
  import { onMount } from 'svelte';
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

  async function bootstrap() {
    const status = await api('GET', '/api/status');
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
  <p class="dim" style="padding: 24px">Contacting server…</p>
{:else if view === 'setup' || view === 'login'}
  <Auth mode={view} onDone={bootstrap} />
{:else}
  <header>
    <h1>DXCA</h1>
    <nav>
      {#each tabs as [id, label]}
        <button class:active={tab === id} onclick={() => (tab = id)}>{label}</button>
      {/each}
    </nav>
    <span class="who">
      <span class="mono">{me.callsign}</span>
      {#if me.role === 'admin'}<span class="dim">admin</span>{/if}
      <button onclick={logout}>Log out</button>
    </span>
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

<style>
  header {
    display: flex;
    align-items: center;
    gap: 20px;
    padding: 10px 16px;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border);
  }
  h1 {
    margin: 0;
    font-size: 17px;
    color: var(--accent);
  }
  nav {
    display: flex;
    gap: 4px;
  }
  nav button {
    background: none;
    border: none;
    color: var(--fg-dim);
    padding: 6px 10px;
    border-radius: 6px;
  }
  nav button.active {
    color: var(--fg);
    background: #21262d;
  }
  nav button:hover {
    color: var(--fg);
  }
  .who {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 10px;
  }
</style>
