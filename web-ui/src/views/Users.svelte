<script lang="ts">
  // Users admin (plan §8 page 6): list + create + edit + delete.
  import { api } from '../lib/api';
  import { onMount } from 'svelte';

  let users = $state<any[]>([]);
  let meId = $state<number | null>(null);
  let callsign = $state('');
  let displayName = $state('');
  let password = $state('');
  let role = $state('user');
  let message = $state('');
  let error = $state('');

  // Row being edited, and its draft. Held apart from `users` so Cancel is a
  // discard rather than a re-fetch, and so a failed Save keeps what was typed.
  let editingId = $state<number | null>(null);
  let draftCall = $state('');
  let draftName = $state('');
  let draftRole = $state('user');
  let draftPass = $state('');
  let rowError = $state('');

  async function load() {
    const r = await api('GET', '/api/users');
    if (r.status === 200) users = r.json.users;
  }
  onMount(async () => {
    const m = await api('GET', '/api/me');
    if (m.status === 200) meId = m.json.id;
    await load();
  });

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

  function startEdit(u: any) {
    editingId = u.id;
    draftCall = u.callsign;
    draftName = u.display_name;
    draftRole = u.role;
    draftPass = '';
    rowError = '';
    message = ''; error = '';
  }

  function cancelEdit() {
    editingId = null;
    rowError = '';
  }

  async function save(u: any) {
    rowError = '';
    // Send only what changed: PATCH leaves absent fields alone, so an
    // untouched password field must not arrive as an empty string.
    const body: Record<string, unknown> = {};
    if (draftCall.trim() !== u.callsign) body.callsign = draftCall.trim();
    if (draftName !== u.display_name) body.display_name = draftName;
    if (draftRole !== u.role) body.role = draftRole;
    if (draftPass) body.password = draftPass;
    if (Object.keys(body).length === 0) { cancelEdit(); return; }

    const r = await api('PATCH', `/api/users/${u.id}`, body);
    if (r.status === 200) {
      message = `Updated ${r.json.user.callsign}.`;
      editingId = null;
      await load();
    } else {
      rowError = r.json?.error ?? `HTTP ${r.status}`;
    }
  }

  async function remove(u: any) {
    const self = u.id === meId;
    const last = users.length === 1;
    let warn = `Delete ${u.callsign}? Their ClubLog settings, alert preferences and worked matrix go too.`;
    if (self) warn += '\n\nThis is your own account — you will be logged out immediately.';
    if (last) warn += '\n\nThis is the last account. The server will return to first-run setup.';
    if (!confirm(warn)) return;

    message = ''; error = ''; rowError = '';
    const r = await api('DELETE', `/api/users/${u.id}`);
    if (r.status === 200) {
      // Own session cascaded away with the row, and at zero accounts the
      // setup card is what should be on screen — either way, reload.
      if (self || r.json.remaining === 0) { location.reload(); return; }
      message = `Deleted ${r.json.deleted}.`;
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
      <thead><tr><th>Callsign</th><th>Name</th><th>Role</th><th></th></tr></thead>
      <tbody>
        {#each users as u}
          <tr>
            {#if editingId === u.id}
              <td><input class="mono" bind:value={draftCall} autocapitalize="characters" /></td>
              <td><input bind:value={draftName} /></td>
              <td>
                <select bind:value={draftRole}>
                  <option value="user">user</option>
                  <option value="admin">admin</option>
                </select>
              </td>
              <td class="row-actions">
                <button class="primary" onclick={() => save(u)}>Save</button>
                <button onclick={cancelEdit}>Cancel</button>
              </td>
            {:else}
              <td class="mono call">{u.callsign}{#if u.id === meId}<span class="you">you</span>{/if}</td>
              <td>{u.display_name}</td>
              <td><span class="pill" class:on={u.role === 'admin'}>{u.role}</span></td>
              <td class="row-actions">
                <button onclick={() => startEdit(u)}>Edit</button>
                <button class="danger" onclick={() => remove(u)}>Delete</button>
              </td>
            {/if}
          </tr>
          {#if editingId === u.id}
            <tr class="edit-extra">
              <td></td>
              <td colspan="3">
                <input
                  type="password"
                  bind:value={draftPass}
                  placeholder="new password (leave blank to keep)"
                />
                {#if rowError}<p class="err">{rowError}</p>{/if}
              </td>
            </tr>
          {/if}
        {/each}
      </tbody>
    </table>
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
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
  </form>
</div>

<style>
  .call {
    font-weight: 600;
  }

  /* Marks the caller's own row: deleting it logs them out, so it should not
     look like any other row. */
  .you {
    margin-left: 0.4rem;
    font-weight: 400;
    font-size: 0.7rem;
    color: var(--muted);
  }

  .row-actions {
    display: flex;
    gap: 0.35rem;
    justify-content: flex-end;
  }

  button.danger {
    color: var(--err);
  }

  button.danger:hover:not(:disabled) {
    border-color: var(--err);
  }

  /* The password field gets its own row: it is the one edit with no column
     of its own, and squeezing it into the role cell made the table jump. */
  .edit-extra input {
    width: 100%;
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
