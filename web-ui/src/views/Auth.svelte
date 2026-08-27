<script lang="ts">
  // Login and first-run setup share one card — `mode` picks the wording.
  import { api } from '../lib/api';
  import ThemeSwitcher from '../lib/ThemeSwitcher.svelte';

  let { mode, onDone }: { mode: 'login' | 'setup'; onDone: () => void } = $props();

  let callsign = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit(ev: Event) {
    ev.preventDefault();
    busy = true;
    error = '';
    const path = mode === 'setup' ? '/api/setup' : '/api/login';
    const r = await api('POST', path, { callsign, password });
    busy = false;
    if (r.status === 200) onDone();
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }
</script>

<div class="wrap">
  <form class="card" onsubmit={submit}>
    <!-- The wordmark stands in for the header the signed-out screens don't
         have, and carries the appearance pick with it: the operator can set
         the theme before they can set anything else. -->
    <div class="brand">
      <h1>DXCA</h1>
      <ThemeSwitcher />
    </div>
    <h2>{mode === 'setup' ? 'Welcome — create the admin account' : 'Log in'}</h2>
    {#if mode === 'setup'}
      <p class="hint intro">
        First run: this account administers DXCA (sources, nodes, users).
      </p>
    {/if}
    <div class="settings-form">
      <span class="label">Callsign</span>
      <input bind:value={callsign} placeholder="VU2CPL" autocapitalize="characters" />
      <span class="label">Password</span>
      <input type="password" bind:value={password} placeholder={mode === 'setup' ? 'min 6 characters' : ''} />
    </div>
    {#if error}<p class="err">{error}</p>{/if}
    <button class="primary submit" disabled={busy}>
      {mode === 'setup' ? 'Create account' : 'Log in'}
    </button>
  </form>
</div>

<style>
  .wrap {
    display: grid;
    place-items: center;
    min-height: 100vh;
    padding: 1.25rem;
  }

  form.card {
    width: min(24rem, 100%);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding-bottom: 0.85rem;
    margin-bottom: 1.1rem;
    border-bottom: 1px solid var(--border);
  }

  /* Pushed to the far end — it is chrome, not part of the wordmark. */
  .brand :global(.theme-widget) {
    margin-left: auto;
  }

  /* A sentence, not a card label: the global h2 is the small uppercase muted
     heading that names a GROUP, and "Welcome — create the admin account" set
     in it reads as shouting rather than as the screen's own title. */
  form.card h2 {
    font-size: 1.05rem;
    text-transform: none;
    letter-spacing: normal;
    color: CanvasText;
    margin-bottom: 1rem;
  }

  .intro {
    margin: -0.65rem 0 1rem;
    line-height: 1.5;
  }

  /* Two short labels on a 24rem card — the app's 9rem gutter would eat a
     third of the width and leave the inputs cramped. */
  .settings-form {
    grid-template-columns: 6.5rem 1fr;
  }

  .submit {
    width: 100%;
    margin-top: 1.1rem;
    padding: 0.4rem 0.7rem;
    font-size: var(--fs-item);
  }

  p.err {
    margin: 0.75rem 0 0;
  }
</style>
