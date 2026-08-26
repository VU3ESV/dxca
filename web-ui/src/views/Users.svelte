<script lang="ts">
  // Users admin (plan §8 page 6): list + create.
  import { api } from '../lib/api';
  import { onMount } from 'svelte';

  let users = $state<any[]>([]);
  let callsign = $state('');
  let displayName = $state('');
  let password = $state('');
  let role = $state('user');
  let message = $state('');
  let error = $state('');

  async function load() {
    const r = await api('GET', '/api/users');
    if (r.status === 200) users = r.json.users;
  }
  onMount(load);

  async function create(ev: Event) {
    ev.preventDefault();
    message = ''; error = '';
    const r = await api('POST', '/api/users', {
      callsign, password, display_name: displayName, role,
    });
    if (r.status === 200) {
      message = `Created ${r.json.callsign}.`;
      callsign = displayName = password = '';
      await load();
    } else {
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }
</script>

<div class="page">
  <div class="card">
    <h2>Users</h2>
    <table>
      <thead><tr><th>Callsign</th><th>Name</th><th>Role</th></tr></thead>
      <tbody>
        {#each users as u}
          <tr><td class="mono">{u.callsign}</td><td>{u.display_name}</td><td>{u.role}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
  <form class="card" onsubmit={create}>
    <h2>Add user</h2>
    <div class="form-row"><span>Callsign</span><input bind:value={callsign} /></div>
    <div class="form-row"><span>Display name</span><input bind:value={displayName} /></div>
    <div class="form-row"><span>Password</span><input type="password" bind:value={password} placeholder="min 6 chars" /></div>
    <div class="form-row">
      <span>Role</span>
      <select bind:value={role}><option value="user">user</option><option value="admin">admin</option></select>
    </div>
    <button class="primary">Create</button>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
  </form>
</div>

<style>
  .page { padding: 20px; display: flex; flex-direction: column; gap: 16px; }
  th { position: static; }
</style>
