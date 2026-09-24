<script>
  import { onMount } from 'svelte'
  import { t } from './lib/i18n.svelte.js'
  import { store, start, dismissToast } from './lib/store.svelte.js'
  import { get, put } from './lib/api.js'
  import Nav from './components/Nav.svelte'
  import Dashboard from './routes/Dashboard.svelte'
  import Library from './routes/Library.svelte'
  import Devices from './routes/Devices.svelte'
  import Logs from './routes/Logs.svelte'
  import Settings from './routes/Settings.svelte'

  const known = ['dashboard', 'library', 'devices', 'logs', 'settings']
  const fromHash = () => {
    const raw = location.hash.replace(/^#\/?/, '')
    return known.includes(raw) ? raw : 'dashboard'
  }

  let tab = $state(fromHash())

  function go(next) {
    tab = next
    if (location.hash !== `#/${next}`) location.hash = `#/${next}`
    window.scrollTo({ top: 0, behavior: 'smooth' })
  }

  /// Jezik se pamti na serveru (`config.ui.language`) da ga i drugi uređaji dobiju.
  async function persistLanguage(choice) {
    try {
      const current = await get('/api/settings')
      const config = { ...current.config, ui: { ...(current.config.ui ?? {}), language: choice } }
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

<div class="app">
  <Nav {tab} ontab={go} onlanguage={persistLanguage} />

  {#if tab === 'dashboard'}<Dashboard onopen={go} />
  {:else if tab === 'library'}<Library />
  {:else if tab === 'devices'}<Devices />
  {:else if tab === 'logs'}<Logs />
  {:else if tab === 'settings'}<Settings />{/if}
</div>

<div class="toasts">
  {#each store.toasts as toast (toast.id)}
    <div class="toast {toast.kind}" onclick={() => dismissToast(toast.id)} title={t('common.cancel')}>
      {toast.text}
    </div>
  {/each}
</div>
