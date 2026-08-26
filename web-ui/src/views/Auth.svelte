<script lang="ts">
  // Login and first-run setup share one card — `mode` picks the wording.
  import { api } from '../lib/api';

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
    <h2>{mode === 'setup' ? 'Welcome — create the admin account' : 'DXCA login'}</h2>
    {#if mode === 'setup'}
      <p class="dim">
        First run: this account administers DXCA (sources, nodes, users).
      </p>
    {/if}
    <div class="form-row">
      <span>Callsign</span>
      <input bind:value={callsign} placeholder="VU2CPL" autocapitalize="characters" />
    </div>
    <div class="form-row">
      <span>Password</span>
      <input type="password" bind:value={password} placeholder={mode === 'setup' ? 'min 6 characters' : ''} />
    </div>
    {#if error}<p class="err">{error}</p>{/if}
    <button class="primary" disabled={busy}>
      {mode === 'setup' ? 'Create account' : 'Log in'}
    </button>
  </form>
</div>

<style>
  .wrap {
    display: grid;
    place-items: center;
    min-height: 80vh;
  }
  form.card {
    width: 360px;
  }
  button {
    margin-top: 6px;
    width: 100%;
  }
</style>
