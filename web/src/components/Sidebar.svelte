<script>
  let restartujem = $state(false)
  async function restartujServis() {
    restartujem = true
    try {
      await fetch('/api/restart', { method: 'POST' })
      // servis se dize ~5 s; pricekaj pa osvjezi stranicu
      setTimeout(() => location.reload(), 7000)
    } catch (greska) {
      restartujem = false
      alert('Restart nije uspio: ' + greska.message)
    }
  }
  // Lijevi stupac: znak, navigacija i stanje servera u podnožju.
  import { t, i18n, setLocale, languages } from '../lib/i18n.svelte.js'
  let osvjezavam = $state(false)
  let osvjezeno = $state(0)

  /* Osvjezi postere/metapodatke za cijelu knjiznicu: korijenske mape
     (Filmovi, Serije) same prolaze kroz svoje podstablo. */
  async function osvjeziSlike() {
    osvjezavam = true
    try {
      const odgovor = await fetch('/api/browse?id=0&limit=50')
      const podaci = await odgovor.json()
      const korijeni = (podaci.items ?? []).filter((s) => s.container).map((s) => s.id)
      let ukupno = 0
      // Korijenska mapa sama ne prima osvjezavanje — idemo po njezinoj djeci
      // (filmovi su stavke, serije su cvorovi koji onda prodju kroz svoje sezone).
      for (const korijen of korijeni) {
        const grana = await fetch(`/api/browse?id=${encodeURIComponent(korijen)}&limit=500`)
        const djeca = await grana.json()
        for (const dijete of djeca.items ?? []) {
          const r = await fetch('/api/metadata/refresh', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ id: dijete.id }),
          })
          const o = await r.json()
          ukupno += o?.found ?? 0
          osvjezeno = ukupno
        }
      }
      osvjezeno = ukupno
    } finally {
      osvjezavam = false
    }
  }

  import { store, rescan, restartServer } from '../lib/store.svelte.js'
  import { uptime } from '../lib/format.js'
  import { TABS } from '../lib/nav.js'
  import Icon from './Icon.svelte'

  let { tab, ontab, onlanguage } = $props()

  const online = $derived(store.logSocket === 'open')
</script>

<aside class="sidebar">
  <div class="brand">
    <span class="logo"><img src="/icon-48.png" alt="Rustiio" /></span>
    <span>
      <!-- Naziv proizvoda se ne prevodi; ključ „app“ je rječnik, ne tekst, pa se ispisivalo [object Object]. -->
      <span class="name">Rustiio</span><br />
      <span class="ver">{store.status?.version ? `v${store.status.version}` : '—'}</span>
    </span>
  </div>

  <nav class="nav">
    {#each TABS as item}
      <button class="nav-item" class:active={tab === item.id} onclick={() => ontab(item.id)}>
        <span class="ico"><Icon name={item.id} /></span>
        {t(`nav.${item.id}`)}
      </button>
    {/each}
  </nav>

  <div class="foot">
    <div class="row tight">
      <span class="dot" class:off={!online} class:live={online}></span>
      <span class="grow small dim">{online ? (t('side.live_log_connected')) : (t('side.log_disconnected'))}</span>
    </div>
    <div class="row tight">
      <span class="small dim">{t('common.uptime')}</span>
      <span class="grow right small num">{uptime(store.status?.uptime_secs)}</span>
    </div>
    <div class="row tight">
      <span class="small dim">{t('side.items')}</span>
      <span class="grow right small num">{store.status?.items ?? '—'}</span>
    </div>

    <div style="display: flex; gap: 6px; margin-top: 10px">
      <select
        class="select small"
        style="flex: 1"
        aria-label={t('common.language')}
        value={i18n.choice}
        onchange={(event) => {
          setLocale(event.currentTarget.value)
          onlanguage?.(event.currentTarget.value)
        }}
      >
        {#each languages as language}
          <option value={language.id}>{language.label()}</option>
        {/each}
      </select>
    </div>
    <button class="btn ghost" style="width: 100%; margin-top: 6px"
      title={t('common.refresh_images_hint')} onclick={osvjeziSlike} disabled={osvjezavam}>
      <Icon name="image" size={14} /> {osvjezavam ? t('common.refresh_images_working') : t('common.refresh_images')}
    </button>
    {#if osvjezeno > 0}
      <div class="tiny faint" style="margin-top: 4px">{t('common.refresh_images_done')}: {osvjezeno}</div>
    {/if}
    <button class="btn scan" title={t('common.scan_hint')} onclick={rescan}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor"
        stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"
        ><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5" /></svg> {t('common.scan')}
    </button>
    <button class="btn ghost" style="width: 100%; margin-top: 6px" title={t('common.restart_hint')}
      onclick={restartServer}>
      ⏻ {t('common.restart')}
    </button>
    <div class="tiny faint" style="margin-top: 4px">{t('common.auto_scan')}
    </div>
    <div class="tiny faint mono" style="margin-top: 8px; word-break: break-all">{store.status?.base_url ?? ''}</div>
  </div>
</aside>