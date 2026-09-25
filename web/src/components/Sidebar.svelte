<script>
  // Lijevi stupac: znak, navigacija i stanje servera u podnožju.
  import { t, i18n, setLocale, languages } from '../lib/i18n.svelte.js'
  import { store, rescan } from '../lib/store.svelte.js'
  import { uptime } from '../lib/format.js'
  import { TABS } from '../lib/nav.js'

  let { tab, ontab, onlanguage } = $props()

  const online = $derived(store.logSocket === 'open')
</script>

<aside class="sidebar">
  <div class="brand">
    <span class="logo"><img src="/icon-48.png" alt="Rustiio" /></span>
    <span>
      <!-- Naziv proizvoda se ne prevodi; `t('app')` vraća cijeli rječnik pa je ispisivao [object Object]. -->
      <span class="name">Rustiio</span><br />
      <span class="ver">{store.status?.version ? `v${store.status.version}` : '—'}</span>
    </span>
  </div>

  <nav class="nav">
    {#each TABS as item}
      <button class="nav-item" class:active={tab === item.id} onclick={() => ontab(item.id)}>
        <span class="ico" aria-hidden="true">{item.icon}</span>
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
        aria-label="Jezik sučelja"
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
      <button class="btn ghost" title={t('common.refresh')} onclick={rescan}>⟳</button>
    </div>
    <div class="tiny faint mono" style="margin-top: 8px; word-break: break-all">{store.status?.base_url ?? ''}</div>
  </div>
</aside>
