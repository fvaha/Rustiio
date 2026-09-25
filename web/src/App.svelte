<script>
  // Okvir: lijevi stupac (navigacija), zaglavlje s naslovom stranice i stanjem,
  // sadržaj stranice i poruke. Na mobitelu navigacija ide u zaglavlje.
  import { onMount } from 'svelte'
  import Icon from './components/Icon.svelte'
  import { t, i18n } from './lib/i18n.svelte.js'
  import { store, start, dismissToast, refreshStatus } from './lib/store.svelte.js'
  import { get, put } from './lib/api.js'
  import { TABS, TAB_IDS } from './lib/nav.js'
  import Sidebar from './components/Sidebar.svelte'
  import Dashboard from './routes/Dashboard.svelte'
  import Library from './routes/Library.svelte'
  import Devices from './routes/Devices.svelte'
  import Logs from './routes/Logs.svelte'
  import Settings from './routes/Settings.svelte'

  // Ruta iz adrese: `#/settings`. Sve ostalo (npr. `#server` iz skoka na sekciju
  // unutar Postavki) **nije** ruta i ne smije mijenjati stranicu — inače klik na
  // sekciju Postavki baci korisnika na Overview.
  const hashRoute = () => {
    // Prazna adresa i `#/` su Pregled (naslovnica).
    if (!location.hash || location.hash === '#' || location.hash === '#/') return 'dashboard'
    const match = location.hash.match(/^#\/([a-z-]+)$/)
    return match && TAB_IDS.includes(match[1]) ? match[1] : null
  }

  let tab = $state(hashRoute() ?? 'dashboard')

  const current = $derived(TABS.find((item) => item.id === tab) ?? TABS[0])
  const online = $derived(store.logSocket === 'open')

  function go(next) {
    tab = next
    if (location.hash !== `#/${next}`) location.hash = `#/${next}`
    window.scrollTo({ top: 0, behavior: 'smooth' })
  }

  /// Jezik se pamti na serveru (`config.ui.language`) da ga dobiju i drugi uređaji.
  async function persistLanguage(choice) {
    try {
      const currentConfig = await get('/api/settings')
      const config = { ...currentConfig.config, ui: { ...(currentConfig.config.ui ?? {}), language: choice } }
      await put('/api/settings', config)
    } catch (error) {
      // Nije kritično: jezik i dalje vrijedi u ovom browseru.
      console.warn('jezik nije spremljen na server:', error)
    }
  }

  onMount(() => {
    start()
    // Mijenjaj stranicu samo kad je u adresi **ruta** (`#/postavke`); skok na
    // sekciju unutar Postavki (`#server`) ne smije vratiti korisnika na Pregled.
    const onHash = () => {
      const route = hashRoute()
      if (route) tab = route
    }
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  })
</script>

<div class="shell">
  <Sidebar {tab} ontab={go} onlanguage={persistLanguage} />

  <div class="main">
    <header class="topbar">
      <div>
        <div class="page-title">{t(`nav.${current.id}`)}</div>
        <div class="page-sub hide-sm">
          {#if tab === 'dashboard'}
            {t('app.live_state_of_the_server_library_and_strea')}
          {:else if tab === 'library'}
            {t('app.what_the_tvs_see_folder_by_folder')}
          {:else if tab === 'devices'}
            {t('app.tvs_that_appeared_and_the_profile_each_one')}
          {:else if tab === 'logs'}
            {t('app.live_server_log_with_levels_and_filtering')}
          {:else}
            {t('app.every_setting_saved_to_config_toml')}
          {/if}
        </div>
      </div>

      <span class="spacer"></span>

      <span class="badge {online ? 'ok' : 'err'}" title={store.status?.base_url}>
        <span class="dot {online ? 'live' : 'off'}"></span>
        {online ? (t('app.connected')) : (t('app.no_log_stream'))}
      </span>
      <button class="btn ghost" title={t('common.refresh')} onclick={refreshStatus}>⟳</button>

      <nav class="mobile-tabs">
        {#each TABS as item}
          <button class="nav-item" class:active={tab === item.id} onclick={() => go(item.id)}>
            <span class="ico"><Icon name={item.id} /></span>
            {t(`nav.${item.id}`)}
          </button>
        {/each}
      </nav>
    </header>

    <main class="page">
      {#if tab === 'dashboard'}<Dashboard onopen={go} />
      {:else if tab === 'library'}<Library />
      {:else if tab === 'devices'}<Devices />
      {:else if tab === 'logs'}<Logs />
      {:else if tab === 'settings'}<Settings />{/if}
    </main>
  </div>
</div>

<div class="toasts">
  {#each store.toasts as toast (toast.id)}
    <div class="toast {toast.kind}" role="status" onclick={() => dismissToast(toast.id)} title={t('common.cancel')}>
      {toast.text}
    </div>
  {/each}
</div>
