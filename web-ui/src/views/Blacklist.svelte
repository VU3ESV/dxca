<script lang="ts">
  // Server-wide call blacklist (admin). A listed call is dropped in the
  // pipeline before the spot ring, so it disappears from every screen, the
  // telnet cluster server, the filtered UDP destinations and Telegram at
  // once — not a display filter, which is what the band/mode chips are for.
  import { api } from '../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../lib/HelpTip.svelte';

  let calls = $state<string[]>([]);
  let callsign = $state('');
  let message = $state('');
  let error = $state('');
  let loaded = $state(false);

  async function load() {
    const r = await api('GET', '/api/blacklist');
    if (r.status === 200) calls = r.json.calls ?? [];
    else error = r.json?.error ?? `HTTP ${r.status}`;
    loaded = true;
  }
  onMount(load);

  async function add(ev: Event) {
    ev.preventDefault();
    message = ''; error = '';
    const r = await api('POST', '/api/blacklist', { callsign });
    if (r.status === 200) {
      calls = r.json.calls;
      // `added: false` means it was already there — worth saying, so the
      // operator does not wonder why the list did not grow.
      message = r.json.added
        ? `${r.json.callsign} blocked.`
        : `${r.json.callsign} was already blocked.`;
      callsign = '';
    } else {
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }

  async function remove(call: string) {
    message = ''; error = '';
    const r = await api('DELETE', `/api/blacklist/${encodeURIComponent(call)}`);
    if (r.status === 200) {
      calls = r.json.calls;
      message = `${r.json.removed} unblocked.`;
    } else {
      error = r.json?.error ?? `HTTP ${r.status}`;
    }
  }
</script>

<div class="settings-pair">
  <div class="card">
    <h2>
      Blocked calls
      <HelpTip label="Blocked calls">
        A call added here is dropped the moment it arrives — it will not appear
        in Spots, will not reach your logger over telnet or the filtered UDP
        destinations, and will not raise a Telegram alert.
      </HelpTip>
    </h2>
    {#if loaded && calls.length === 0}
      <p class="hint">Nothing blocked.</p>
    {:else}
      <table>
        <thead><tr><th>Callsign</th><th></th></tr></thead>
        <tbody>
          {#each calls as call}
            <tr>
              <td class="mono call">{call}</td>
              <td class="row-actions">
                <button onclick={() => remove(call)}>Unblock</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
    {#if message}<p class="ok">{message}</p>{/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>

  <form class="card" onsubmit={add}>
    <h2>
      Block a call
      <HelpTip label="Block a call">
        <span class="para">
          Matched exactly against the spotted station's callsign,
          case-insensitively. No wildcards: <code>R1ABC</code> blocks that call
          and nothing else.
        </span>
        <span class="para">
          One list for the whole server — it applies to every account, because
          the spot is discarded before anyone sees it. The verbatim UDP
          passthrough is the single exception: it forwards decoder datagrams
          untouched, before any parsing, so a blocked call can still reach a
          logger that way.
        </span>
      </HelpTip>
    </h2>
    <div class="settings-form">
      <span class="label">Callsign</span>
      <input bind:value={callsign} autocapitalize="characters" placeholder="e.g. R1ABC" />
    </div>
    <div class="actions">
      <button class="primary">Block</button>
    </div>
  </form>
</div>

<style>
  .call {
    font-weight: 600;
  }

  .row-actions {
    display: flex;
    justify-content: flex-end;
  }

  .settings-form {
    grid-template-columns: 7.5rem 1fr;
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
