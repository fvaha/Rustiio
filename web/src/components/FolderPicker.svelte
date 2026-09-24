<script>
  // Biranje mape s videom: pregleda datotečni sustav servera, korisnik samo klikne.
  // Isto radi u browseru i u desktop aplikaciji jer je sučelje jedno te isto.
  let { open = false, start = '', onpick = () => {}, onclose = () => {}, lang = 'hr' } = $props()

  const hr = {
    title: 'Odaberi mapu s videom',
    here: 'videa u ovoj mapi',
    inside: 'podmapa',
    videos: 'videa',
    pick: 'Odaberi ovu mapu',
    cancel: 'Odustani',
    up: 'Roditeljska mapa',
    empty: 'Nema podmapa.',
    loading: 'Čitam mapu…',
    shortcuts: 'Brzi put',
    chosen: 'Odabrano',
    notAbsolute: 'Putanja mora biti apsolutna (počinje s „/"). Odaberi mapu iz popisa.',
  }
  const en = {
    title: 'Pick the folder with your video',
    here: 'videos in this folder',
    inside: 'subfolders',
    videos: 'videos',
    pick: 'Use this folder',
    cancel: 'Cancel',
    up: 'Parent folder',
    empty: 'No subfolders.',
    loading: 'Reading folder…',
    shortcuts: 'Shortcuts',
    chosen: 'Selected',
    notAbsolute: 'The path must be absolute (start with "/"). Pick a folder from the list.',
  }
  const t = $derived(lang === 'en' ? en : hr)

  let path = $state('')
  let listing = $state(null)
  let error = $state('')
  let busy = $state(false)

  // Mrvice moraju dati **apsolutnu** putanju — inače se u config spremi
  // "Users/vaha/…" i server ga odbije ("mapa mora biti apsolutna putanja").
  let segments = $derived.by(() => {
    const raw = String(path ?? '')
    const parts = raw.split(/[/\\]/).filter(Boolean)
    const windows = /^[A-Za-z]:/.test(raw)
    const root = windows ? `${parts[0]}\\` : '/'
    const rest = windows ? parts.slice(1) : parts
    return rest.map((name, index) => ({ name, path: root + rest.slice(0, index + 1).join('/') }))
  })

  const isAbsolute = (candidate) => String(candidate ?? '').startsWith('/') || /^[A-Za-z]:[/\\]/.test(String(candidate ?? ''))

  async function load(target) {
    busy = true
    error = ''
    try {
      const query = target ? `?path=${encodeURIComponent(target)}` : ''
      const response = await fetch(`/api/fs/list${query}`, { headers: { accept: 'application/json' } })
      if (!response.ok) throw new Error(`HTTP ${response.status}`)
      listing = await response.json()
      path = listing.path ?? ''
      error = listing.error ?? ''
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }

  $effect(() => {
    if (open) load(start || '')
  })

  function onKey(event) {
    if (event.key === 'Escape' && open) onclose()
  }

  /// Odabir je moguć samo s apsolutnom putanjom — server takvu i traži.
  function useFolder() {
    if (!isAbsolute(path)) {
      error = t.notAbsolute
      return
    }
    onpick(path)
    onclose()
  }
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <div class="picker-backdrop" role="presentation" onclick={(event) => event.target === event.currentTarget && onclose()}>
    <div class="picker" role="dialog" aria-modal="true" aria-label={t.title}>
      <header class="picker-head">
        <b>{t.title}</b>
        <button class="btn ghost" type="button" title={t.cancel} onclick={onclose}>✕</button>
      </header>

      <div class="picker-path mono" title={path}>{path}</div>

      <div class="picker-crumbs">
        {#each segments as segment, index}
          <button class="picker-crumb" type="button" onclick={() => load(segment.path)}>{segment.name}</button>
          {#if index < segments.length - 1}<span class="picker-sep">›</span>{/if}
        {/each}
        {#if listing?.parent}
          <button class="btn ghost sm" type="button" title={t.up} onclick={() => load(listing.parent)}>↑</button>
        {/if}
        <span class="picker-count">
          {t.here}: {listing?.videos ?? 0}
        </span>
      </div>

      {#if listing?.shortcuts?.length}
        <div class="picker-short">
          <span class="picker-label">{t.shortcuts}</span>
          {#each listing.shortcuts as shortcut}
            <button class="picker-chip" type="button" onclick={() => load(shortcut.path)}>{shortcut.name}</button>
          {/each}
        </div>
      {/if}

      {#if error}
        <p class="picker-error">⚠ {error}</p>
      {/if}

      <div class="picker-list">
        {#if busy && !listing?.dirs?.length}
          <p class="hint">{t.loading}</p>
        {:else if !listing?.dirs?.length}
          <p class="hint">{t.empty}</p>
        {:else}
          {#each listing.dirs as dir}
            <button class="picker-row" type="button" onclick={() => load(dir.path)}>
              <svg class="picker-icon" viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
                <path d="M1.5 3.5h4.2l1.3 1.6h7.5v7.4a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1V3.5z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" />
              </svg>
              <span class="picker-name">{dir.name}</span>
              <span class="picker-meta">
                {#if dir.videos}<span class="badge ok">{t.videos}: {dir.videos}</span>{/if}
                {#if dir.folders}<span class="badge">{t.inside}: {dir.folders}</span>{/if}
              </span>
            </button>
          {/each}
        {/if}
      </div>

      <footer class="picker-foot">
        <span class="picker-picked mono">{t.chosen}: {path}</span>
        <span class="spacer"></span>
        <button class="btn" type="button" onclick={onclose}>{t.cancel}</button>
        <button class="btn primary" type="button" disabled={busy} onclick={useFolder}>
          {t.pick}
        </button>
      </footer>
    </div>
  </div>
{/if}
