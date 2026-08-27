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

<!-- Two short cards: the masonry columns let the roster and the form sit
     side by side on a wide window and stack on a narrow one. -->
<div class="page card-grid">
  <div class="card">
    <h2>Users</h2>
    <table>
      <thead><tr><th>Callsign</th><th>Name</th><th>Role</th></tr></thead>
      <tbody>
        {#each users as u}
          <tr>
            <td class="mono call">{u.callsign}</td>
            <td>{u.display_name}</td>
            <td><span class="pill" class:on={u.role === 'admin'}>{u.role}</span></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <form class="card" onsubmit={create}>
    <h2>Add user</h2>
    <div class="settings-form">
      <span class="label">Callsign</span>
      <input bind:value={callsign} autocapitalize="characters" />
      <span class="label">Display name</span>
      <input bind:value={displayName} />
      <span class="label">Password</span>
      <input type="password" bind:value={password} placeholder="min 6 chars" />
      <span class="label">Role</span>
      <select bind:value={role}><option value="user">user</option><option value="admin">admin</option></select>
    </div>
    <div class="actions">
      <button class="primary">Create</button>
    </div>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
  </form>
</div>

<style>
  .call {
    font-weight: 600;
  }

  /* Four short labels — the app's 9rem column would leave a trench between
     them and their fields inside a masonry column this narrow. */
  .settings-form {
    grid-template-columns: 7.5rem 1fr;
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
