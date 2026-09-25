<script>
  // Knjižnica: mape, posteri, pretraga i "pusti". Isti sadržaj koji TV vidi
  // (`/api/browse`), pa nema razlike između onoga što TV pokaže i ovoga.
  import { t } from '../lib/i18n.svelte.js'
  import { get, post } from '../lib/api.js'
  import { toast } from '../lib/store.svelte.js'
  import { bytes, dateTime } from '../lib/format.js'

  let cwd = $state('0')
  let trail = $state([{ id: '0', title: t('nav.library') }])
  let items = $state([])
  let total = $state(0)
  let query = $state('')
  let kind = $state('')
  let loading = $state(false)
  let selected = $state(null)

  async function load(id = cwd) {
    loading = true
    try {
      const params = new URLSearchParams({ id, limit: '500' })
      if (query.trim()) params.set('q', query.trim())
      if (kind) params.set('kind', kind)
      const data = await get(`/api/browse?${params}`)
      items = data.items ?? []
      total = data.total ?? 0
      cwd = data.id
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
      items = []
    } finally {
      loading = false
    }
  }

  function openFolder(item) {
    trail = [...trail, { id: item.id, title: item.title }]
    selected = null
    load(item.id)
  }

  function jumpTo(index) {
    const target = trail[index]
    trail = trail.slice(0, index + 1)
    selected = null
    load(target.id)
  }

  // Popis se sam osvjezava (auto-sken doda nove filmove i bez klika).
  $effect(() => {
    const sat = setInterval(() => load(), 60_000)
    return () => clearInterval(sat)
  })

  /// Ponovno razriješi naslov i poster (isto za film i za seriju).
  async function osvjeziMetapodatke(item, dogadjaj) {
    dogadjaj?.stopPropagation?.()
    try {
      const odgovor = await post('/api/metadata/refresh', { id: item.id })
      toast('ok', `${t('library.refresh_meta')}: ${odgovor?.found ?? 0}`)
      await load()
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }

  async function copyLink(item) {
    const url = `${location.origin}${item.play ?? `/art/${item.id}`}`
    try {
      await navigator.clipboard.writeText(url)
      toast('ok', `${t('common.copied')}: ${url}`)
    } catch {
      toast('warn', url)
    }
  }

  let searchTimer
  function onSearch() {
    clearTimeout(searchTimer)
    searchTimer = setTimeout(() => load(), 250)
  }

  $effect(() => {
    load('0')
  })
</script>

<div class="lib-bar">
  <input class="field" style="flex: 1 1 220px" placeholder={t('library.search_placeholder')} bind:value={query} oninput={onSearch} />
  <select class="field" bind:value={kind} onchange={() => load()}>
    <option value="">{t('common.all')}</option>
    <option value="video">{t('common.video')}</option>
    <option value="audio">{t('common.audio')}</option>
    <option value="image">{t('common.image')}</option>
    <option value="folder">{t('common.folder')}</option>
  </select>
  <span class="pill">{total} {t('library.items_count')}</span>
</div>

<div class="crumbs" style="margin-bottom: 12px">
  {#each trail as step, index}
    {#if index > 0}<span>›</span>{/if}
    <button onclick={() => jumpTo(index)}>{step.title}</button>
  {/each}
  {#if query}<span class="tag accent">{t('common.search')}: {query}</span>{/if}
</div>

{#if loading}
  <div class="empty">{t('common.loading')}</div>
{:else if items.length === 0}
  <div class="empty">{t('library.empty')}</div>
{:else}
  <div class="posters">
    {#each items as item (item.id)}
      <!-- Jedna vrsta kartice za sve: serija ima poster kao i film — dosad je
           serija (container) crtala samo 🗂, pa je poster iz API-ja propadao. -->
      <div
        class="poster"
        class:open={item.container}
        onclick={() => (item.container ? openFolder(item) : (selected = selected?.id === item.id ? null : item))}
        role="button"
        tabindex="0"
        onkeydown={(event) => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault()
            item.container ? openFolder(item) : (selected = item)
          }
        }}
      >
        <button class="meta-refresh" title={t('library.refresh_meta_hint')}
          onclick={(dogadjaj) => osvjeziMetapodatke(item, dogadjaj)}>⟳</button>
        {#if item.poster}
          <img class="img" src={item.poster} alt={item.title} loading="lazy" />
        {:else}
          <div class="img none">{item.container ? '🗂' : '🎞'}</div>
        {/if}
        <div class="cap">
          <div class="name" title={item.title}>{item.title}</div>
          <div class="sub">
            {#if item.container}
              {item.children} {t('library.items_count')}
            {:else}
              {bytes(item.size)} · {item.kind}
            {/if}
          </div>
        </div>
        <!-- Titlovi po jeziku: `English`, `Croatian (forced)` — nikad "Language 1". -->
        {#each item.subtitles ?? [] as titl (titl.file)}
          <span class="tag" title={titl.file}>{titl.name}</span>
        {/each}
      </div>
    {/each}
  </div>
{/if}

{#if selected}
  <div class="card" style="position: fixed; right: 16px; bottom: 16px; width: min(420px, 92vw); z-index: 50">
    <div class="card-head">
      <span class="card-title">{selected.title}</span>
      <span class="card-actions">
        <button class="btn ghost" onclick={() => (selected = null)}>✕</button>
      </span>
    </div>
    <div class="card-body">
      <div class="row"><span class="grow sub">{selected.putanja}</span></div>
      <div class="row"><span class="grow sub">{t('library.filter_kind')}</span><span class="tag">{selected.kind}</span></div>
      <div class="row"><span class="grow sub">{t('library.size')}</span><span class="tag">{bytes(selected.size)}</span></div>
      {#if (selected.subtitles ?? []).length}
        <div class="row">
          <span class="grow sub">{t('library.subtitles')}</span>
          <span class="tag">
            {selected.subtitles.map((titl) => titl.name).join(', ')}
          </span>
        </div>
      {/if}
      <div class="row"><span class="grow sub">{t('library.added')}</span><span class="tag">{dateTime(selected.modified_ms)}</span></div>
      <div style="display: flex; gap: 8px; margin-top: 12px; flex-wrap: wrap">
        {#if selected.play}
          <a class="btn primary" href={selected.play} target="_blank" rel="noreferrer">{t('library.play')}</a>
        {/if}
        <button class="btn" onclick={() => copyLink(selected)}>{t('library.copy_link')}</button>
        <button class="btn" onclick={() => osvjeziMetapodatke(selected)}>{t('library.refresh_meta')}</button>
        <button class="btn" disabled title={t('library.on_tv_later')} style="opacity: 0.5">
          {t('library.on_tv')}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  :global(.poster) {
    position: relative;
  }
  .meta-refresh {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 2;
    width: 26px;
    height: 26px;
    line-height: 1;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: rgba(0, 0, 0, 0.55);
    color: var(--foreground);
    cursor: pointer;
  }
  .meta-refresh:hover {
    background: var(--accent);
    color: #fff;
  }
</style>
