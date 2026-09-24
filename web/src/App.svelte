<script>
  // Okvir: lijevi stupac (navigacija), zaglavlje s naslovom stranice i stanjem,
  // sadržaj stranice i poruke. Na mobitelu navigacija ide u zaglavlje.
  import { onMount } from 'svelte'
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

  const fromHash = () => {
    const raw = location.hash.replace(/^#\/?/, '')
    return TAB_IDS.includes(raw) ? raw : 'dashboard'
  }

  let tab = $state(fromHash())

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
    const onHash = () => (tab = fromHash())
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
            {i18n.lang === 'en' ? 'Live state of the server, library and streams' : 'Stanje servera, knjižnice i strimova u živo'}
          {:else if tab === 'library'}
            {i18n.lang === 'en' ? 'What the TVs see, folder by folder' : 'Ono što televizori vide, mapa po mapa'}
          {:else if tab === 'devices'}
            {i18n.lang === 'en' ? 'TVs that appeared, and the profile each one gets' : 'Televizori koji su se pojavili i profil koji svaki dobiva'}
          {:else if tab === 'logs'}
            {i18n.lang === 'en' ? 'Live server log with levels and filtering' : 'Zapisnik servera u živo, s razinama i filtriranjem'}
          {:else}
            {i18n.lang === 'en' ? 'Every setting, saved to config.toml' : 'Sva podešavanja, spremaju se u config.toml'}
          {/if}
        </div>
      </div>

      <span class="spacer"></span>

      <span class="badge {online ? 'ok' : 'err'}" title={store.status?.base_url}>
        <span class="dot {online ? 'live' : 'off'}"></span>
        {online ? (i18n.lang === 'en' ? 'connected' : 'spojeno') : (i18n.lang === 'en' ? 'no log stream' : 'nema toka')}
      </span>
      <button class="btn ghost" title={t('common.refresh')} onclick={refreshStatus}>⟳</button>

      <nav class="mobile-tabs">
        {#each TABS as item}
          <button class="nav-item" class:active={tab === item.id} onclick={() => go(item.id)}>
            <span class="ico" aria-hidden="true">{item.icon}</span>
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
