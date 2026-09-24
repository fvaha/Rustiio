<script>
  // Sken sustava u Postavkama → Transkodiranje.
  // Jedno mjesto gdje se vidi što stroj ima i gdje se podesi čime se prekodira.
  import { get, post } from '../lib/api.js'
  import { t } from '../lib/i18n.svelte.js'

  let sken = $state(null)
  let uPogonu = $state(null)
  let status = $state('')
  let greska = $state('')
  let radi = $state(false)
  const nacini = [
    { key: 'auto', label: () => t('tr.mode_auto') },
    { key: 'gpu', label: () => t('tr.mode_gpu') },
    { key: 'cpu', label: () => t('tr.mode_cpu') },
    { key: 'hybrid', label: () => t('tr.mode_hybrid') },
  ]
  let nacin = $state('auto')

  async function skeniraj() {
    radi = true
    greska = ''
    status = ''
    try {
      const odgovor = await get('/api/transcode/scan')
      sken = odgovor.sken
      uPogonu = odgovor.u_pogonu
      if (odgovor.sken?.recommendation?.mode) nacin = odgovor.sken.recommendation.mode
    } catch (problem) {
      greska = problem.message ?? String(problem)
    } finally {
      radi = false
    }
  }

  async function primijeni(pustinac = false) {
    radi = true
    greska = ''
    status = ''
    try {
      const odgovor = await post('/api/transcode/apply', { mode: nacin })
      uPogonu = { ...(uPogonu ?? {}), ...(odgovor.preporuka ?? {}) }
      if (odgovor.greska) {
        greska = odgovor.greska
      } else {
        status = odgovor.promjene?.length
          ? t('tr.saved_changes') + ' ' + odgovor.promjene.join(' · ')
          : t('tr.saved_nothing')
      }
      if (pustinac) await skeniraj()
    } catch (problem) {
      greska = problem.message ?? String(problem)
    } finally {
      radi = false
    }
  }

  const postotak = (probe) => {
    if (!probe?.fps || !sken?.encoders?.length) return 0
    const naj = Math.max(...sken.encoders.map((item) => item.fps ?? 0), 1)
    return Math.max(4, Math.round(((probe.fps ?? 0) / naj) * 100))
  }
</script>

<div class="scan">
  <div class="vrh">
    <div>
      <h3>{t('tr.scan_title')}</h3>
      <p>{t('tr.scan_help')}</p>
    </div>
    <div class="gumbi">
      <button type="button" onclick={skeniraj} disabled={radi}>
        {radi ? t('tr.scanning') : t('tr.scan_button')}
      </button>
      <button type="button" class="glavni" onclick={() => primijeni(true)} disabled={radi || !sken}>
        {t('tr.apply_button')}
      </button>
    </div>
  </div>

  {#if greska}
    <p class="greska">{greska}</p>
  {/if}
  {#if status}
    <p class="uspjeh">{status} {t('tr.restart_needed')}</p>
  {/if}

  {#if !sken}
    <p class="prazno">{t('tr.scan_hint')}</p>
  {:else}
    <div class="kutije">
      <div class="kutija">
        <span class="oznaka">{t('tr.cpu')}</span>
        <strong>{sken.cpu.model}</strong>
        <span class="sitno">{sken.cpu.threads} {t('tr.threads')}</span>
      </div>
      <div class="kutija">
        <span class="oznaka">{t('tr.gpu')}</span>
        {#if sken.gpus.length}
          {#each sken.gpus as gpu}
            <strong>{gpu.name}</strong>
            <span class="sitno">{gpu.note}{gpu.memory_mb ? ` · ${Math.round(gpu.memory_mb / 1024)} GB` : ''}</span>
          {/each}
        {:else}
          <strong>{t('tr.gpu_none')}</strong>
          <span class="sitno">{t('tr.gpu_none_help')}</span>
        {/if}
      </div>
      <div class="kutija">
        <span class="oznaka">ffmpeg</span>
        {#each sken.ffmpeg.filter((alat) => alat.works) as alat}
          <strong class="mono">{alat.path}</strong>
          <span class="sitno mono">{alat.version ?? ''}</span>
        {/each}
        {#if !sken.ffmpeg.some((alat) => alat.works)}
          <strong class="greska">{t('tr.ffmpeg_missing')}</strong>
        {/if}
        <span class="sitno">{sken.subtitles_filter ? t('tr.burn_ok') : t('tr.burn_no')}</span>
      </div>
    </div>

    <table class="enkoderi">
      <thead>
        <tr>
          <th>{t('tr.encoder')}</th>
          <th>{t('tr.kind')}</th>
          <th>{t('tr.speed')}</th>
        </tr>
      </thead>
      <tbody>
        {#each sken.encoders as probe}
          <tr class={probe.works ? '' : 'ne'}>
            <td class="mono">{probe.encoder}</td>
            <td>{probe.hw === 'none' ? t('tr.sw') : probe.hw}</td>
            <td>
              {#if probe.works && probe.fps}
                <span class="traka"><i style="width: {postotak(probe)}%"></i></span>
                <span class="sitno">{Math.round(probe.fps)} {t('tr.fps')}</span>
              {:else}
                <span class="sitno">{probe.note}</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>

    <div class="nacin">
      <span class="oznaka">{t('tr.mode')}</span>
      {#each nacini as opcija}
        <label class:odabran={nacin === opcija.key}>
          <input type="radio" name="tr-nacin" value={opcija.key} bind:group={nacin} />
          {opcija.label()}
        </label>
      {/each}
      <button type="button" onclick={() => primijeni(false)} disabled={radi}>{t('tr.save_mode')}</button>
    </div>

    {#if sken.recommendation}
      <p class="preporuka" title={sken.recommendation.reason}>
        <strong>{t('tr.recommendation')}:</strong>
        <span class="mono">{sken.recommendation.encoder}</span>
        {#if sken.recommendation.threads}· {sken.recommendation.threads} {t('tr.threads')}{/if}
        {#if sken.recommendation.hardware_decode}· {t('tr.hw_decode')}{/if}
        {#if sken.recommendation.best_fps}
          — {t('tr.measured')} {Math.round(sken.recommendation.best_fps)} {t('tr.fps')}
          {#if sken.recommendation.other_fps}
            / {Math.round(sken.recommendation.other_fps)} {t('tr.fps')}
          {/if}
        {/if}
      </p>
    {/if}
    {#if uPogonu}
      <p class="sitno">
        {t('tr.in_use')}: <span class="mono">{uPogonu.encoder ?? t('tr.auto')}</span>
        {#if uPogonu.threads}· {uPogonu.threads} {t('tr.threads')}{/if}
        {#if uPogonu.hardware_decode}· {t('tr.hw_decode')}{/if}
      </p>
    {/if}
  {/if}
</div>

<style>
  .scan {
    margin-top: 1rem;
    border: 1px solid var(--border, #2a2f3a);
    border-radius: 12px;
    padding: 0.9rem 1rem;
    background: color-mix(in srgb, var(--card, #14171d) 88%, transparent);
  }
  .vrh {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    flex-wrap: wrap;
    align-items: flex-start;
  }
  h3 {
    margin: 0 0 0.2rem;
    font-size: 1rem;
  }
  p {
    margin: 0.2rem 0;
    font-size: 0.85rem;
    color: var(--muted-foreground, #9aa4b2);
  }
  .gumbi {
    display: flex;
    gap: 0.5rem;
  }
  button {
    border: 1px solid var(--border, #2a2f3a);
    background: transparent;
    color: inherit;
    border-radius: 8px;
    padding: 0.45rem 0.8rem;
    cursor: pointer;
    font: inherit;
  }
  button.glavni {
    background: #22c55e;
    border-color: #22c55e;
    color: #05130a;
    font-weight: 600;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .kutije {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 0.6rem;
    margin: 0.8rem 0;
  }
  .kutija {
    border: 1px solid var(--border, #2a2f3a);
    border-radius: 10px;
    padding: 0.6rem 0.7rem;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .oznaka {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted-foreground, #9aa4b2);
  }
  .sitno {
    font-size: 0.75rem;
    color: var(--muted-foreground, #9aa4b2);
  }
  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.8rem;
    word-break: break-all;
  }
  .enkoderi {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85rem;
    margin-bottom: 0.7rem;
  }
  .enkoderi th {
    text-align: left;
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted-foreground, #9aa4b2);
    padding: 0.2rem 0.4rem;
  }
  .enkoderi td {
    padding: 0.25rem 0.4rem;
    border-top: 1px solid var(--border, #2a2f3a);
  }
  .enkoderi tr.ne {
    opacity: 0.45;
  }
  .traka {
    display: inline-block;
    width: 110px;
    height: 8px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--border, #2a2f3a) 70%, transparent);
    overflow: hidden;
    vertical-align: middle;
    margin-right: 0.4rem;
  }
  .traka i {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, #16a34a, #4ade80);
  }
  .nacin {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    flex-wrap: wrap;
    margin: 0.4rem 0 0.6rem;
  }
  .nacin label {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.85rem;
    border: 1px solid var(--border, #2a2f3a);
    border-radius: 999px;
    padding: 0.25rem 0.65rem;
    cursor: pointer;
  }
  .nacin label.odabran {
    border-color: #22c55e;
    color: #4ade80;
  }
  .nacin input {
    accent-color: #22c55e;
  }
  .preporuka,
  .prazno {
    font-size: 0.85rem;
  }
  .greska {
    color: #f87171;
  }
  .uspjeh {
    color: #4ade80;
  }
</style>
