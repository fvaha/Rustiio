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

  // DD+/eac3 naslovi: napravi kopiju s AC-3 zvukom (slika se kopira 1:1).
  let poslovi = $state([])
  function mjestoURedu(posao) {
    const cekaju = poslovi.filter((p) => p.state === 'ceka')
    const i = cekaju.findIndex((p) => p.redni === posao.redni)
    return i >= 0 ? i + 1 : 1
  }
  async function pripremiAc3(item, dogadjaj) {
    dogadjaj?.stopPropagation?.()
    try {
      const odgovor = await post('/api/prepare/ac3', { id: item.id })
      const greska = `${odgovor?.greska ?? ''}`
      if (odgovor?.ok === false && /postoji/i.test(greska)) {
        gotovi = new Set(gotovi).add(item.id) // kopija je vec tu — nije greska
        toast('ok', 'AC-3 kopija već postoji — ništa ne treba raditi.')
      } else if (odgovor?.ok === false) {
        toast('err', greska || 'greška')
      } else {
        toast('ok', odgovor.poruka ?? `U redu: ${odgovor.out ?? ''}`)
      }
      await osvjeziPoslove()
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }
  async function osvjeziPoslove() {
    try {
      const podaci = await get('/api/prepare/status')
      poslovi = podaci.poslovi ?? []
    } catch {
      /* server jos nema tu rutu - ne smeta */
    }
  }
  $effect(() => {
    osvjeziPoslove()
    const sat = setInterval(osvjeziPoslove, 5000)
    return () => clearInterval(sat)
  })

  // Kratko na posteru: sitno "DD+ → AC-3" dugme samo na naslovima s eac3 zvukom.
  // Za mapu do 60 videa pitamo /api/decision/<id> (server to kesira) i pokazemo gumb.
  let ddplus = $state(new Set())
  let gotovi = $state(new Set())
  let ddplusKljuc = $state('')
  $effect(() => {
    const vids = (items ?? []).filter((i) => !i.container && i.kind === 'video').map((i) => i.id)
    if (!vids.length || vids.length > 60) {
      ddplus = new Set()
      return
    }
    let ziv = true
    Promise.all(vids.map((id) => get(`/api/decision/${encodeURIComponent(id)}`).catch(() => null))).then((odluke) => {
      if (!ziv) return
      const novi = new Set()
      odluke.forEach((odluka, i) => {
        const kodek = `${odluka?.media?.audio?.codec ?? ''}`.toLowerCase()
        const razlog = `${odluka?.reasons ?? ''}`.toLowerCase()
        if (kodek.includes('eac3') || razlog.includes('eac3')) novi.add(vids[i])
      })
      ddplus = novi
    })
    return () => {
      ziv = false
    }
  })

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
        <!-- Titlovi po jeziku: `English`, `Croatian (forced)` — nikad "Language 1".
             U vlastitom kontejneru da se ne nabijaju jedan na drugi kad ih je vise. -->
        {#if gotovi.has(item.id)}
          <div class="subs"><span class="dd-stanje">AC-3 ✓ (kopija postoji)</span></div>
        {:else if ddplus.has(item.id)}
          <div class="subs">
            {#each poslovi.filter((posao) => posao.id === item.id).slice(0, 1) as posao (posao.redni)}
              <span class="dd-stanje" class:greska={posao.state === 'greska'} title={posao.message}>
                {#if posao.state === 'radi'}
                  DD+ → AC-3 · radi…
                {:else if posao.state === 'ceka'}
                  U redu — {mjestoURedu(posao)}. na redu
                {:else if posao.state === 'gotovo'}
                  AC-3 ✓
                {:else}
                  greska: {posao.message}
                {/if}
              </span>
            {:else}
              <button
                class="dd-btn"
                title="DD+ zvuk: napravi kopiju s AC-3 (slika se kopira 1:1) i pusti je direktno"
                onclick={(dogadjaj) => pripremiAc3(item, dogadjaj)}
              >
                DD+ → AC-3
              </button>
            {/each}
          </div>
        {/if}
        {#if (item.subtitles ?? []).length}
          <div class="subs">
            {#each item.subtitles ?? [] as titl (titl.file)}
              <span class="sub-tag" title={titl.file}>{titl.name}</span>
            {/each}
          </div>
        {/if}
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
      {#if poslovi.length}
        <div class="poslovi">
          {#each poslovi.slice(0, 3) as posao (`${posao.id}-${posao.started_ms}`)}
            <div class="row">
              <span class="grow sub" title={posao.out}>{posao.title}</span>
              <span class="tag" style={posao.state === 'greska' ? 'color: var(--err)' : ''}>
                {posao.state === 'radi' ? 'radi…' : posao.state}{posao.message ? `: ${posao.message}` : ''}
              </span>
            </div>
          {/each}
        </div>
      {/if}
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
  /* Titlovi na kartici: uredan red koji se lomi, nikad preklapanje. */
  :global(.poster) .subs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 0 10px 10px;
    margin-top: auto;
  }
  .poslovi {
    margin-top: 10px;
    border-top: 1px solid var(--line);
    padding-top: 8px;
  }
  :global(.poster) .dd-btn,
  :global(.poster) .dd-stanje {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    width: 100%;
    padding: 9px 14px;
    border-radius: 10px;
    border: 1px solid color-mix(in srgb, var(--accent) 55%, transparent);
    background: linear-gradient(
      180deg,
      color-mix(in srgb, var(--accent) 30%, transparent),
      color-mix(in srgb, var(--accent) 14%, transparent)
    );
    color: var(--foreground);
    font-size: 13.5px;
    font-weight: 700;
    letter-spacing: 0.2px;
    line-height: 1.35;
    cursor: pointer;
    white-space: normal;
    text-align: center;
    box-shadow: 0 3px 12px rgba(0, 0, 0, 0.35);
    transition: background 0.15s, transform 0.15s, box-shadow 0.15s;
  }
  :global(.poster) .dd-btn:hover {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
    transform: translateY(-1px);
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.45);
  }
  :global(.poster) .dd-stanje {
    cursor: default;
    font-weight: 600;
    font-size: 12.5px;
    border-color: var(--line-2);
    background: var(--surface-3);
    color: var(--faint);
    box-shadow: none;
    overflow-wrap: anywhere;
  }
  :global(.poster) .dd-stanje.greska {
    border-color: var(--err);
    color: var(--err);
    background: color-mix(in srgb, var(--err) 12%, transparent);
  }
  :global(.poster) .dd-btn:hover {
    background: var(--accent);
    color: #fff;
  }
  :global(.poster) .dd-stanje {
    cursor: default;
    border-color: var(--line-2);
    background: var(--surface-3);
    color: var(--faint);
  }
  :global(.poster) .dd-stanje.greska {
    border-color: var(--err);
    color: var(--err);
  }
  :global(.poster) .sub-tag {
    display: inline-flex;
    align-items: center;
    max-width: 100%;
    padding: 2px 7px;
    border-radius: 999px;
    background: var(--surface-3);
    border: 1px solid var(--line-2);
    color: var(--faint);
    font-size: 10.5px;
    font-family: var(--mono);
    line-height: 1.5;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
