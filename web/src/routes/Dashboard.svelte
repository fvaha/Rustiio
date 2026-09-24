<script>
  // Pregled: računalo, strimovi, knjižnica, posteri, diskovi, uređaji, prekodiranje, zapisnik.
  // Kartice se povlače, mijenjaju širinu i skupljaju (lib/layout.svelte.js).
  import { t, i18n } from '../lib/i18n.svelte.js'
  import { store, refreshPosters, saveSettings, toast } from '../lib/store.svelte.js'
  import { get } from '../lib/api.js'
  import { cardOf, resetLayout } from '../lib/layout.svelte.js'
  import { bytes, percent, uptime, bitrate, barClass, ago, duration } from '../lib/format.js'
  import Card from '../components/Card.svelte'
  import Spark from '../components/Spark.svelte'
  import FolderPicker from '../components/FolderPicker.svelte'

  let { onopen } = $props()

  let posters = $state(null)
  let picking = $state(false)
  let saving = $state('')
  // Jezik sučelja dolazi iz zajedničkog stanja (i18n.lang je 'hr' ili 'en').
  const lang = $derived(i18n.lang)
  const hr = $derived(lang === 'hr')

  /// Dodaj mapu s videom izravno s pregleda: spremi i, ako treba, restartaj.
  async function addFolder(path) {
    picking = false
    saving = path
    try {
      // `GET /api/settings` vraća omotnicu (`config`, `ceka_restart`, `putanja`) —
      // na server se šalje samo `config`, inače validacija padne na `udn`.
      const { config } = await get('/api/settings')
      const roots = Array.isArray(config?.library?.roots) ? config.library.roots : []
      const name = String(path).split(/[/\\]/).filter(Boolean).pop() ?? path
      if (roots.some((root) => root.path === path)) {
        saving = ''
        return
      }
      config.library.roots = [...roots, { label: name, path, kind: 'video' }]
      const result = await saveSettings(config)
      // Ne restartaj sam: u desktop aplikaciji bi to ugasilo i prozor, a na
      // serveru ovisi o tome kako je usluga postavljena. Reci korisniku što treba.
      if (result?.restart_potreban) {
        toast('warn', t('dash.folders_saved_a_restart_is_needed_reopen_t'), 12000)
      }
    } catch (error) {
      toast('err', String(error?.message ?? error))
    } finally {
      saving = ''
    }
  }

  /// Ukloni mapu koje više nema na disku.
  async function dropFolder(path) {
    try {
      const { config } = await get('/api/settings')
      const roots = Array.isArray(config?.library?.roots) ? config.library.roots : []
      config.library.roots = roots.filter((root) => root.path !== path)
      const result = await saveSettings(config)
      if (result?.restart_potreban) {
        toast('warn', t('dash.folders_saved_a_restart_is_needed'), 12000)
      }
    } catch (error) {
      toast('err', String(error?.message ?? error))
    }
  }

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
  <Card id="system" title={t('dash.machine')} span={spanOf('system', 8)} collapsed={collapsedOf('system')}>
    {#snippet actions()}
      <span class="badge">{stats?.cpu?.brand ?? '—'}</span>
      <span class="badge info">{stats?.cpu?.cores ?? 0} {t('dash.cores')}</span>
    {/snippet}

    <div class="metrics">
      <div class="metric accent">
        <b>{percent(stats?.cpu?.usage ?? 0)}</b>
        <span>{t('dash.cpu')} · {t('dash.load')} {(stats?.cpu?.load?.[0] ?? 0).toFixed(2)}</span>
      </div>
      <div class="metric ok">
        <b>{percent(memoryPercent)}</b>
        <span>{t('dash.memory')} · {bytes(stats?.memory?.used)} / {bytes(stats?.memory?.total)}</span>
      </div>
      <div class="metric">
        <b>{uptime(stats?.uptime_s ?? 0)}</b>
        <span>{t('dash.uptime')}</span>
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
        <div class="tiny faint" style="margin-bottom: 4px">{t('dash.cpu_last_minutes')}</div>
        <Spark values={store.history.cpu} max={100} height={44} id="cpu" color="var(--accent)" />
      </div>
      <div>
        <div class="tiny faint" style="margin-bottom: 4px">{t('dash.memory_last_minutes')}</div>
        <Spark values={store.history.memory} max={100} height={44} id="ram" color="var(--ok)" />
      </div>
    </div>

    <div class="row tight" style="margin-top: 10px">
      <span class="grow small dim">Rustiio (pid {stats?.process?.pid ?? '—'})</span>
      <span class="badge">{bytes(stats?.process?.memory)} · {percent(stats?.process?.cpu ?? 0, 1)}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{t('dash.discovery_and_subscriptions')}</span>
      <span class="badge" class:ok={status?.ssdp} class:err={!status?.ssdp}>{status?.ssdp ? 'SSDP ✓' : 'SSDP ✕'}</span>
      <span class="badge">{status?.subscriptions ?? 0} {t('dash.subscriptions')}</span>
    </div>
  </Card>

  <!-- strimovi -->
  <Card id="streams" title={t('dash.active_streams')} span={spanOf('streams', 4)} collapsed={collapsedOf('streams')}>
    {#snippet actions()}
      <span class="badge info">{store.streams?.active ?? 0}/{store.streams?.max_concurrent ?? 0}</span>
    {/snippet}

    {#if streamList.length === 0}
      <div class="empty"><span class="ico">◌</span>{t('dash.no_tv_is_playing_right_now')}</div>
    {:else}
      {#each streamList as stream (stream.id)}
        <div class="row">
          <span class="grow">
            <div class="name">{stream.target || stream.object_id}</div>
            <div class="sub">{stream.device || '—'} · {streamDuration(stream)}{#if stream.bitrate} · {bitrate(stream.bitrate)}{/if}</div>
          </span>
          <span class="badge" class:info={stream.mode === 'transcode'} class:ok={stream.mode !== 'transcode'}>
            {stream.mode === 'transcode' ? (t('dash.transcode')) : (t('dash.direct'))}
          </span>
        </div>
        <div class="row tight">
          <span class="grow tiny faint mono">{stream.encoder ?? '—'}</span>
        </div>
      {/each}
    {/if}

    <div class="row tight" style="margin-top: 8px">
      <span class="grow small dim">{t('dash.free_slots')}</span>
      <span class="badge">{store.streams?.available_slots ?? 0}</span>
    </div>
  </Card>

  <!-- knjižnica -->
  <Card id="library" title={t('dash.library')} span={spanOf('library', 4)} collapsed={collapsedOf('library')}>
    {#snippet actions()}
      <button class="btn ghost" title={t('dash.show_listing')} onclick={() => onopen?.('library')}>→</button>
    {/snippet}
    <div class="metrics">
      <div class="metric"><b>{status?.items ?? 0}</b><span>{t('dash.items')}</span></div>
      <div class="metric"><b>{counts.videos ?? 0}</b><span>{t('dash.video')}</span></div>
      <div class="metric"><b>{counts.folders ?? 0}</b><span>{t('dash.folders')}</span></div>
    </div>
    {#each status?.roots ?? [] as root}
      <div class="row tight">
        <span class="grow small">{root.label || root.path}</span>
        {#if !root.exists}
          <button
            class="btn ghost danger sm"
            type="button"
            title={t('dash.remove_a_folder_that_is_gone')}
            onclick={() => dropFolder(root.path)}
          >✕</button>
        {/if}
        <span class="badge tiny" class:ok={root.exists} class:err={!root.exists}>{root.exists ? '✓' : (t('dash.missing'))}</span>
      </div>
    {:else}
      <div class="empty">{t('dash.no_folder_configured')}</div>
    {/each}
    <div class="row tight">
      <button class="btn primary sm" type="button" disabled={!!saving} onclick={() => (picking = true)}>
        + {t('dash.add_video_folder')}
      </button>
      {#if saving}<span class="grow small dim">{t('dash.saving')}</span>{/if}
    </div>

    <FolderPicker open={picking} {lang} onpick={addFolder} onclose={() => (picking = false)} />
    <div class="row tight">
      <span class="grow small dim">{t('dash.virtual_folders')}</span>
      <span class="badge" class:ok={status?.views}>{status?.views ? (t('dash.on')) : (t('dash.off'))}</span>
    </div>
  </Card>

  <!-- posteri -->
  <Card id="posters" title={t('dash.posters')} span={spanOf('posters', 4)} collapsed={collapsedOf('posters')}>
    {#snippet actions()}
      <span class="badge" class:ok={posters?.tmdb_key} class:warn={!posters?.tmdb_key}>{posters?.tmdb_key ? 'TMDB' : (t('dash.no_key'))}</span>
      <button class="btn ghost" title={t('dash.check_now')} onclick={refreshPosters}>⟳</button>
    {/snippet}
    <div class="metrics">
      <div class="metric ok"><b>{posters?.have ?? 0}</b><span>{t('dash.have')}</span></div>
      <div class="metric warn"><b>{posters?.pending ?? 0}</b><span>{t('dash.waiting')}</span></div>
      <div class="metric err"><b>{posters?.none ?? 0}</b><span>{t('dash.none')}</span></div>
    </div>
    <div class="bar lg" style="margin-top: 12px">
      <i class="ok" style="width: {posterPercent}%"></i>
    </div>
    <div class="tiny faint" style="margin-top: 6px">
      {percent(posterPercent)} {t('dash.coverage')} · {posterTotal} {t('dash.titles')}
    </div>
  </Card>

  <!-- diskovi -->
  <Card id="disks" title={t('dash.disks')} span={spanOf('disks', 4)} collapsed={collapsedOf('disks')}>
    {#each (stats?.disks ?? []).slice(0, 6) as disk (disk.montirano)}
      <div class="row">
        <span class="grow">
          <div class="name small">{disk.montirano}</div>
          <div class="bar" style="margin-top: 6px"><i class={barClass(disk.posto)} style="width: {Math.min(100, disk.posto)}%"></i></div>
        </span>
        <span class="right nowrap">
          <div class="small num">{percent(disk.posto)}</div>
          <div class="tiny faint num">{bytes(disk.slobodno)} {t('dash.free')}</div>
        </span>
      </div>
    {:else}
      <div class="empty">{t('dash.no_disk_data')}</div>
    {/each}
  </Card>

  <!-- uređaji -->
  <Card id="devices" title={t('dash.devices')} span={spanOf('devices', 6)} collapsed={collapsedOf('devices')}>
    {#snippet actions()}
      <span class="badge">{store.devices?.count ?? 0}</span>
      <button class="btn ghost" title={t('dash.all_devices')} onclick={() => onopen?.('devices')}>→</button>
    {/snippet}
    {#if deviceList.length === 0}
      <div class="empty"><span class="ico">◌</span>{t('dash.no_device_has_connected_yet')}</div>
    {:else}
      {#each deviceList as device (device.key)}
        <div class="row">
          <span class="grow">
            <div class="name">{device.friendly_name || device.user_agent || device.ip}</div>
            <div class="sub mono">{device.ip} · {device.requests} {t('dash.requests')} · {ago(device.last_seen * 1000)}</div>
          </span>
          <span class="badge violet">{device.profile}</span>
        </div>
      {/each}
    {/if}
  </Card>

  <!-- prekodiranje -->
  <Card id="transcode" title={t('dash.transcoding')} span={spanOf('transcode', 6)} collapsed={collapsedOf('transcode')}>
    {#snippet actions()}
      <span class="badge" class:ok={status?.transcode_enabled} class:err={!status?.transcode_enabled}>
        {status?.transcode_enabled ? (t('dash.on_2')) : (t('dash.off_2'))}
      </span>
    {/snippet}
    <div class="row tight">
      <span class="grow small dim">{t('dash.acceleration')}</span>
      <span class="badge info">{store.streams?.hw ?? status?.transcode?.hw ?? '—'}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{t('dash.available_encoders')}</span>
      <span class="badge">{(store.streams?.encoders ?? status?.transcode?.encoders ?? []).join(', ') || '—'}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{t('dash.concurrent_limit')}</span>
      <span class="badge">{store.streams?.max_concurrent ?? status?.transcode?.max_concurrent ?? 0}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{t('dash.running_now')}</span>
      <span class="badge" class:info={(store.streams?.active ?? 0) > 0}>{store.streams?.active ?? 0}</span>
    </div>
    <div class="row tight">
      <span class="grow small dim">{t('dash.metadata_probed')}</span>
      <span class="badge">{status?.media_probed ?? 0}</span>
    </div>
  </Card>

  <!-- zapisnik -->
  <Card id="logs" title={t('dash.log')} span={spanOf('logs', 12)} collapsed={collapsedOf('logs')}>
    {#snippet actions()}
      <span class="badge" class:ok={store.logSocket === 'open'} class:err={store.logSocket !== 'open'}>
        <span class="dot {store.logSocket === 'open' ? 'live' : 'off'}"></span>
        {store.logSocket === 'open' ? (t('dash.live')) : (t('dash.offline'))}
      </span>
      <button class="btn ghost" title={t('dash.full_log')} onclick={() => onopen?.('logs')}>→</button>
    {/snippet}
    <div class="logs" style="max-height: 260px">
      {#each logs as line, index (index)}
        <div class="log" class:error={line.level === 'ERROR'} class:warn={line.level === 'WARN'}>
          <span class="t">{line.at}</span>
          <span class="lvl {line.level}">{line.level}</span>
          <span class="msg">{line.message}</span>
        </div>
      {:else}
        <div class="empty">{t('dash.the_log_is_empty')}</div>
      {/each}
    </div>
  </Card>

  <div style="grid-column: 1 / -1; display: flex; justify-content: flex-end">
    <button class="btn ghost" onclick={resetLayout}>{t('dash.reset_card_layout')}</button>
  </div>
</div>
