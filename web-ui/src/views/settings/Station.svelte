<script lang="ts">
  // Settings › My station › Locator & grey line.
  //
  // Its own page rather than a corner of the ClubLog one: it is station data,
  // not a credential, and it drives a feature (the band mask) that has nothing
  // to do with a log.
  import { api } from '../../lib/api';
  import { onMount } from 'svelte';
  import HelpTip from '../../lib/HelpTip.svelte';

  let cfg = $state<any>({ locator: '', greyline_window_min: 45 });
  let message = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    const r = await api('GET', '/api/config/me/station');
    if (r.status === 200 && r.json) cfg = { ...cfg, ...r.json };
  });

  async function save() {
    busy = true; message = ''; error = '';
    // The server validates the locator, and a rejected one must surface as an
    // error rather than a silent no-op — an operator who typed a grid and saw
    // "Saved." would reasonably assume it took.
    const r = await api('PUT', '/api/config/me/station', {
      locator: cfg.locator ?? '',
      greyline_window_min: Number(cfg.greyline_window_min) || 45,
    });
    busy = false;
    if (r.status === 200) message = 'Saved.';
    else error = r.json?.error ?? `HTTP ${r.status}`;
  }

  /// 5 minutes to 6 hours, matching the server's own bound in
  /// `put_station` — a stepper that offers a value the server will refuse is
  /// worse than one that stops.
  const MIN_MIN = 5;
  const MAX_MIN = 360;
  const nudge = (by: number) =>
    (cfg.greyline_window_min = Math.min(
      MAX_MIN,
      Math.max(MIN_MIN, Number(cfg.greyline_window_min || 45) + by),
    ));

  /// Long windows read better in hours — "360" is arithmetic, "6h" is not.
  let asHours = $derived(
    Number(cfg.greyline_window_min) >= 90
      ? `${(Number(cfg.greyline_window_min) / 60).toFixed(1).replace(/\.0$/, '')} h`
      : '',
  );
</script>

<div class="card">
  <h2>Locator &amp; grey line</h2>
  <div class="settings-form">
    <span class="label">
      Locator
      <HelpTip label="Locator">
        Your Maidenhead square, 4 or 6 characters. Optional, and used for one
        thing only: working out where the sun is at your station, which powers
        the <b>band mask</b> on the Spots screen. Leave it blank and nothing
        changes anywhere.
      </HelpTip>
    </span>
    <!-- Six characters wide, because that is all a locator can be. A grid
         square stretched across the card invites the operator to type an
         address into it. -->
    <input
      class="locator"
      bind:value={cfg.locator}
      placeholder="MK82"
      autocapitalize="characters"
      maxlength="6"
    />

    <!-- Only offered once there is a locator: without one there is no sunrise
         to be either side of. -->
    {#if cfg.locator}
      <span class="label">
        Grey line
        <HelpTip label="Grey line">
          How long either side of sunrise and sunset counts as <b>grey line</b>
          — the window when the low bands come alive and the high bands are
          still open. Yours to set, because how long it stays useful varies
          with the band, the season and the path — on the low bands a
          high-latitude path in midwinter can stay enhanced for hours either
          side. <b>5 minutes to 6 hours</b>; 45 is the default, and matches
          Meridian.
        </HelpTip>
      </span>
      <!-- A stepper rather than a bare number field: this is a value the
           operator is meant to nudge and watch, not type once. -->
      <span class="stepper">
        <button type="button" onclick={() => nudge(-5)} aria-label="Narrow the grey-line window by 5 minutes">−</button>
        <input class="mins" type="number" min={MIN_MIN} max={MAX_MIN} step="5" bind:value={cfg.greyline_window_min} />
        <button type="button" onclick={() => nudge(5)} aria-label="Widen the grey-line window by 5 minutes">+</button>
        <span class="unit">min{#if asHours}<span class="hours">· {asHours}</span>{/if}</span>
      </span>
    {/if}
  </div>

  <div class="actions">
    <button class="primary" onclick={save} disabled={busy}>Save</button>
  </div>
  {#if message}<p class="ok">{message}</p>{/if}
  {#if error}<p class="err">{error}</p>{/if}
</div>

<style>
  .locator {
    max-width: 8rem;
    text-transform: uppercase;
    font-family: var(--mono);
  }

  .stepper {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .stepper button {
    width: 1.7rem;
    padding: 0.1rem 0;
    line-height: 1.2;
  }

  .stepper .mins {
    width: 4.5rem;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .stepper .unit {
    color: var(--muted);
    font-size: 0.85rem;
  }

  .stepper .hours {
    margin-left: 0.35rem;
    font-variant-numeric: tabular-nums;
  }

  p {
    margin: 0.75rem 0 0;
  }
</style>
