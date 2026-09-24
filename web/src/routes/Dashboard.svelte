<script>
  // Pregled: računalo, strimovi, knjižnica, posteri, diskovi, uređaji, prekodiranje, zapisnik.
  // Kartice se povlače, mijenjaju širinu i skupljaju (lib/layout.svelte.js).
  import { i18n } from '../lib/i18n.svelte.js'
  import { store, refreshPosters } from '../lib/store.svelte.js'
  import { get } from '../lib/api.js'
  import { cardOf, resetLayout } from '../lib/layout.svelte.js'
  import { bytes, percent, uptime, bitrate, barClass, ago, duration } from '../lib/format.js'
  import Card from '../components/Card.svelte'
  import Spark from '../components/Spark.svelte'

  let { onopen } = $props()

  let posters = $state(null)
  const lang = $derived(i18n.lang === 'en' ? 'en' : 'hr')
  const hr = $derived(lang === 'hr')

  async function loadPosters() {
    try {
      posters = await get('/api/posters')
    } catch {
      /* kartica ostaje prazna; grešku prikazuje store */
    }
  }

  $effect(() => {
    loadPosters()
    const id = setInterval(loadPosters, 20000)
    return () => clearInterval(id)
  })

  const status = $derived(store.status)
  const stats = $derived(store.stats)
  const counts = $derived(status?.counts ?? {})
  const streamList = $derived(store.streams?.streams ?? [])
  const deviceList = $derived((store.devices?.devices ?? []).slice(0, 6))
  const memoryPercent = $derived(stats?.memory?.total ? (stats.memory.used / stats.memory.total) * 100 : 0)
  const posterTotal = $derived((posters?.have ?? 0) + (posters?.pending ?? 0) + (posters?.none ?? 0))
  const posterPercent = $derived(posterTotal ? ((posters?.have ?? 0) / posterTotal) * 100 : 0)
  const logs = $derived(store.logs.slice(-14).reverse())

  /// Koliko traje stream (`started_at` je u sekundama ili milisekundama).
  function streamDuration(stream) {
    const started = Number(stream.started_at) || 0
    const seconds = started > 1e12 ? started / 1000 : started
    return duration(Math.max(0, Date.now() / 1000 - seconds))
  }

  const spanOf = (id, fallback) => cardOf(id)?.span ?? fallback
  const collapsedOf = (id) => cardOf(id)?.collapsed ?? false
</script>

<div class="grid bento">
  <!-- računalo -->
  <Card id="system" title={hr ? 'Računalo' : 'Machine'} span={spanOf('system', 8)} collapsed={collapsedOf('system')}>
    {#snippet actions()}
      <span class="badge">{stats?.cpu?.brand ?? '—'}</span>
      <span class="badge info">{stats?.cpu?.cores ?? 0} {hr ? 'jezgre' : 'cores'}</span>
    {/snippet}

    <div class="metrics">
      <div class="metric accent">
        <b>{percent(stats?.cpu?.usage ?? 0)}</b>
        <span>{hr ? 'Procesor' : 'CPU'} · {hr ? 'opterećenje' : 'load'} {(stats?.cpu?.load?.[0] ?? 0).toFixed(2)}</span>
      </div>
      <div class="metric ok">
        <b>{percent(memoryPercent)}</b>
        <span>{hr ? 'Memorija' : 'Memory'} · {bytes(stats?.memory?.used)} / {bytes(stats?.memory?.total)}</span>
      </div>
      <div class="metric">
        <b>{uptime(stats?.uptime_s ?? 0)}</b>
        <span>{hr ? 'Radi neprekidno' : 'Uptime'}</span>
      </div>
      {#if stats?.gpu}
        <div class="metric violet">
          <b>{stats.gpu.temp ?? '—'}<span class="unit">°C</span></b>
          <span>{stats.gpu.ime}</span>
        </div>
      {/if}
    </div>

    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 14px; margin-top: 16px">
      <div>
        <div class="tiny faint" style="margin-bottom: 4px">{hr ? 'Procesor — zadnjih minuta' : 'CPU — last minutes'}</div>
        <Spark values={store.history.cpu} max={100} height={44} id="cpu" color="var(--accent)" />
      </div>
      <div>
        <div class="tiny faint" style="margin-bottom: 4px">{hr ? 'Memorija — zadnjih minuta' : 'Memory — last minutes'}</div>
        <Spark values={store.history.memory} max={100} height={44} id="ram" color="var(--ok)" />
      </div>
    </div>

    <div class="row tight" style="margin-top: 10px">
      <span class="grow small dim">Rustiio (pid {stats?.process?.pid ?? '—'})</span>
      <span class="badge">{bytes(stats?.process?.memory)} · {percent(stats?.process?.cpu ?? 0, 1)}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{hr ? 'Oglasi i pretplate' : 'Discovery and subscriptions'}</span>
      <span class="badge" class:ok={status?.ssdp} class:err={!status?.ssdp}>{status?.ssdp ? 'SSDP ✓' : 'SSDP ✕'}</span>
      <span class="badge">{status?.subscriptions ?? 0} {hr ? 'pretplata' : 'subscriptions'}</span>
    </div>
  </Card>

  <!-- strimovi -->
  <Card id="streams" title={hr ? 'Aktivni strimovi' : 'Active streams'} span={spanOf('streams', 4)} collapsed={collapsedOf('streams')}>
    {#snippet actions()}
      <span class="badge info">{store.streams?.active ?? 0}/{store.streams?.max_concurrent ?? 0}</span>
    {/snippet}

    {#if streamList.length === 0}
      <div class="empty"><span class="ico">◌</span>{hr ? 'Nijedan televizor trenutačno ne gleda.' : 'No TV is playing right now.'}</div>
    {:else}
      {#each streamList as stream (stream.id)}
        <div class="row">
          <span class="grow">
            <div class="name">{stream.target || stream.object_id}</div>
            <div class="sub">{stream.device || '—'} · {streamDuration(stream)}{#if stream.bitrate} · {bitrate(stream.bitrate)}{/if}</div>
          </span>
          <span class="badge" class:info={stream.mode === 'transcode'} class:ok={stream.mode !== 'transcode'}>
            {stream.mode === 'transcode' ? (hr ? 'Prekodira' : 'Transcode') : (hr ? 'Izravno' : 'Direct')}
          </span>
        </div>
        <div class="row tight">
          <span class="grow tiny faint mono">{stream.encoder ?? '—'}</span>
        </div>
      {/each}
    {/if}

    <div class="row tight" style="margin-top: 8px">
      <span class="grow small dim">{hr ? 'Slobodnih mjesta' : 'Free slots'}</span>
      <span class="badge">{store.streams?.available_slots ?? 0}</span>
    </div>
  </Card>

  <!-- knjižnica -->
  <Card id="library" title={hr ? 'Knjižnica' : 'Library'} span={spanOf('library', 4)} collapsed={collapsedOf('library')}>
    {#snippet actions()}
      <button class="btn ghost" title={hr ? 'Prikaži popis' : 'Show listing'} onclick={() => onopen?.('library')}>→</button>
    {/snippet}
    <div class="metrics">
      <div class="metric"><b>{status?.items ?? 0}</b><span>{hr ? 'objekata' : 'items'}</span></div>
      <div class="metric"><b>{counts.videos ?? 0}</b><span>{hr ? 'video' : 'video'}</span></div>
      <div class="metric"><b>{counts.folders ?? 0}</b><span>{hr ? 'mapa' : 'folders'}</span></div>
    </div>
    {#each status?.roots ?? [] as root}
      <div class="row tight">
        <span class="grow small">{root.label || root.path}</span>
        <span class="badge tiny" class:ok={root.exists} class:err={!root.exists}>{root.exists ? '✓' : (hr ? 'nema mape' : 'missing')}</span>
      </div>
    {:else}
      <div class="empty">{hr ? 'Nijedna mapa nije zadana.' : 'No folder configured.'}</div>
    {/each}
    <div class="row tight">
      <span class="grow small dim">{hr ? 'Virtualne mape' : 'Virtual folders'}</span>
      <span class="badge" class:ok={status?.views}>{status?.views ? (hr ? 'uključene' : 'on') : (hr ? 'isključene' : 'off')}</span>
    </div>
  </Card>

  <!-- posteri -->
  <Card id="posters" title={hr ? 'Naslovnice' : 'Posters'} span={spanOf('posters', 4)} collapsed={collapsedOf('posters')}>
    {#snippet actions()}
      <span class="badge" class:ok={posters?.tmdb_key} class:warn={!posters?.tmdb_key}>{posters?.tmdb_key ? 'TMDB' : (hr ? 'bez ključa' : 'no key')}</span>
      <button class="btn ghost" title={hr ? 'Provjeri sada' : 'Check now'} onclick={refreshPosters}>⟳</button>
    {/snippet}
    <div class="metrics">
      <div class="metric ok"><b>{posters?.have ?? 0}</b><span>{hr ? 'imaju' : 'have'}</span></div>
      <div class="metric warn"><b>{posters?.pending ?? 0}</b><span>{hr ? 'čeka' : 'waiting'}</span></div>
      <div class="metric err"><b>{posters?.none ?? 0}</b><span>{hr ? 'nema' : 'none'}</span></div>
    </div>
    <div class="bar lg" style="margin-top: 12px">
      <i class="ok" style="width: {posterPercent}%"></i>
    </div>
    <div class="tiny faint" style="margin-top: 6px">
      {percent(posterPercent)} {hr ? 'pokrivenosti' : 'coverage'} · {posterTotal} {hr ? 'naslova' : 'titles'}
    </div>
  </Card>

  <!-- diskovi -->
  <Card id="disks" title={hr ? 'Diskovi' : 'Disks'} span={spanOf('disks', 4)} collapsed={collapsedOf('disks')}>
    {#each (stats?.disks ?? []).slice(0, 6) as disk (disk.montirano)}
      <div class="row">
        <span class="grow">
          <div class="name small">{disk.montirano}</div>
          <div class="bar" style="margin-top: 6px"><i class={barClass(disk.posto)} style="width: {Math.min(100, disk.posto)}%"></i></div>
        </span>
        <span class="right nowrap">
          <div class="small num">{percent(disk.posto)}</div>
          <div class="tiny faint num">{bytes(disk.slobodno)} {hr ? 'slobodno' : 'free'}</div>
        </span>
      </div>
    {:else}
      <div class="empty">{hr ? 'Nema podataka o diskovima.' : 'No disk data.'}</div>
    {/each}
  </Card>

  <!-- uređaji -->
  <Card id="devices" title={hr ? 'Uređaji' : 'Devices'} span={spanOf('devices', 6)} collapsed={collapsedOf('devices')}>
    {#snippet actions()}
      <span class="badge">{store.devices?.count ?? 0}</span>
      <button class="btn ghost" title={hr ? 'Svi uređaji' : 'All devices'} onclick={() => onopen?.('devices')}>→</button>
    {/snippet}
    {#if deviceList.length === 0}
      <div class="empty"><span class="ico">◌</span>{hr ? 'Još se nijedan uređaj nije javio.' : 'No device has connected yet.'}</div>
    {:else}
      {#each deviceList as device (device.key)}
        <div class="row">
          <span class="grow">
            <div class="name">{device.friendly_name || device.user_agent || device.ip}</div>
            <div class="sub mono">{device.ip} · {device.requests} {hr ? 'zahtjeva' : 'requests'} · {ago(device.last_seen * 1000)}</div>
          </span>
          <span class="badge violet">{device.profile}</span>
        </div>
      {/each}
    {/if}
  </Card>

  <!-- prekodiranje -->
  <Card id="transcode" title={hr ? 'Prekodiranje' : 'Transcoding'} span={spanOf('transcode', 6)} collapsed={collapsedOf('transcode')}>
    {#snippet actions()}
      <span class="badge" class:ok={status?.transcode_enabled} class:err={!status?.transcode_enabled}>
        {status?.transcode_enabled ? (hr ? 'uključeno' : 'on') : (hr ? 'isključeno' : 'off')}
      </span>
    {/snippet}
    <div class="row tight">
      <span class="grow small dim">{hr ? 'Ubrzanje' : 'Acceleration'}</span>
      <span class="badge info">{store.streams?.hw ?? status?.transcode?.hw ?? '—'}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{hr ? 'Dostupni enkoderi' : 'Available encoders'}</span>
      <span class="badge">{(store.streams?.encoders ?? status?.transcode?.encoders ?? []).join(', ') || '—'}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{hr ? 'Istovremeno najviše' : 'Concurrent limit'}</span>
      <span class="badge">{store.streams?.max_concurrent ?? status?.transcode?.max_concurrent ?? 0}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{hr ? 'U tijeku' : 'Running now'}</span>
      <span class="badge" class:info={(store.streams?.active ?? 0) > 0}>{store.streams?.active ?? 0}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{hr ? 'Metapodaci izmjereni' : 'Metadata probed'}</span>
      <span class="badge">{status?.media_probed ?? 0}</span>
    </div>
  </Card>

  <!-- zapisnik -->
  <Card id="logs" title={hr ? 'Zapisnik' : 'Log'} span={spanOf('logs', 12)} collapsed={collapsedOf('logs')}>
    {#snippet actions()}
      <span class="badge" class:ok={store.logSocket === 'open'} class:err={store.logSocket !== 'open'}>
        <span class="dot {store.logSocket === 'open' ? 'live' : 'off'}"></span>
        {store.logSocket === 'open' ? (hr ? 'uživo' : 'live') : (hr ? 'prekinuto' : 'offline')}
      </span>
      <button class="btn ghost" title={hr ? 'Cijeli zapisnik' : 'Full log'} onclick={() => onopen?.('logs')}>→</button>
    {/snippet}
    <div class="logs" style="max-height: 260px">
      {#each logs as line, index (index)}
        <div class="log" class:error={line.level === 'ERROR'} class:warn={line.level === 'WARN'}>
          <span class="t">{line.at}</span>
          <span class="lvl {line.level}">{line.level}</span>
          <span class="msg">{line.message}</span>
        </div>
      {:else}
        <div class="empty">{hr ? 'Zapisnik je prazan.' : 'The log is empty.'}</div>
      {/each}
    </div>
  </Card>

  <div style="grid-column: 1 / -1; display: flex; justify-content: flex-end">
    <button class="btn ghost" onclick={resetLayout}>{hr ? 'Vrati raspored kartica' : 'Reset card layout'}</button>
  </div>
</div>
