<script>
  // Zaglavlje: kartice + stanje servera u jednom redu (na mobitelu se prelama).
  import { t, i18n, setLocale, languages } from '../lib/i18n.svelte.js'
  import { store, rescan } from '../lib/store.svelte.js'
  import { uptime } from '../lib/format.js'

  let { tab, ontab, onlanguage } = $props()

  const tabs = ['dashboard', 'library', 'devices', 'logs', 'settings']
  const online = $derived(store.logSocket === 'open')
</script>

<header class="top">
  <div class="brand">
    {t('app')}
    <small class="hide-sm">{store.status?.version ? `v${store.status.version}` : ''}</small>
  </div>

  <nav class="tabs">
    {#each tabs as item}
      <button class="tab" aria-current={tab === item ? 'page' : undefined} onclick={() => ontab(item)}>
        {t(`nav.${item}`)}
      </button>
    {/each}
  </nav>

  <span class="spacer"></span>

  <span class="pill hide-sm" title={store.status?.base_url}>
    <span class="dot" class:off={!online}></span>
    {store.status ? `${store.status.items} ${t('dashboard.items')}` : '—'}
  </span>
  <span class="pill hide-sm">{t('common.uptime')} <b>{uptime(store.status?.uptime_secs)}</b></span>

  <select
    class="field"
    style="padding: 5px 8px; font-size: 13px"
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

  <button class="btn" onclick={rescan} title={t('common.refresh')}>⟳ <span class="hide-sm">{t('common.refresh')}</span></button>
</header>
