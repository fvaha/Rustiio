<script>
  // Pregled: streamovi, računalo, knjižnica, diskovi, posteri, zapisnik.
  // Kartice se povlače i mijenjaju širinu (lib/layout.svelte.js).
  import { t } from '../lib/i18n.svelte.js'
  import { store, refreshPosters } from '../lib/store.svelte.js'
  import { get } from '../lib/api.js'
  import { layout, cardOf, resetLayout } from '../lib/layout.svelte.js'
  import { bytes, percent, uptime, bitrate, barClass, ago, duration } from '../lib/format.js'
  import Card from '../components/Card.svelte'
  import Spark from '../components/Spark.svelte'

  let posters = $state(null)

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
  const memoryPercent = $derived(stats?.memory?.total ? (stats.memory.used / stats.memory.total) * 100 : 0)
  const posterTotal = $derived((posters?.have ?? 0) + (posters?.pending ?? 0) + (posters?.none ?? 0))
  const posterPercent = $derived(posterTotal ? ((posters?.have ?? 0) / posterTotal) * 100 : 0)

  /// Koliko traje stream (started_at je u sekundama ili milisekundama).
  function streamDuration(stream) {
    const started = Number(stream.started_at) || 0
    const seconds = started > 1e12 ? started / 1000 : started
    return duration(Math.max(0, Date.now() / 1000 - seconds))
  }
</script>

<div class="bento">
  <Card id="streams" title={t('dashboard.streams')} span={cardOf('streams')?.span ?? 5} collapsed={cardOf('streams')?.collapsed}>
    {#snippet actions()}
      <span class="tag">{store.streams?.hw ?? '—'}</span>
      <span class="tag accent">{store.streams?.available_slots ?? 0} {t('dashboard.slots')}</span>
    {/snippet}

    {#if streamList.length === 0}
      <div class="empty">{t('dashboard.no_streams')}</div>
    {:else}
      {#each streamList as stream (stream.id)}
        <div class="row">
          <span class="grow">
            <div class="name">{stream.target || stream.object_id}</div>
            <div class="sub">
              {stream.device || '—'} · {stream.encoder || '—'} · {streamDuration(stream)}
            </div>
          </span>
          <span class="tag" class:accent={stream.mode === 'transcode'}>
            {stream.mode === 'transcode' ? t('dashboard.transcode') : t('dashboard.direct')}
          </span>
        </div>
      {/each}
    {/if}
  </Card>

  <Card id="system" title={t('dashboard.system')} span={cardOf('system')?.span ?? 4} collapsed={cardOf('system')?.collapsed}>
    <div class="kpis">
      <div class="kpi">
        <b>{percent(stats?.cpu?.usage ?? 0)}</b>
        <span>{t('dashboard.cpu')} · {stats?.cpu?.cores ?? 0} {t('dashboard.cores')}</span>
      </div>
      <div class="kpi">
        <b>{percent(memoryPercent)}</b>
        <span>{t('dashboard.memory')} · {bytes(stats?.memory?.used)} / {bytes(stats?.memory?.total)}</span>
      </div>
      <div class="kpi">
        <b>{uptime(stats?.uptime_s ?? 0)}</b>
        <span>{t('common.uptime')}</span>
      </div>
    </div>

    <div style="margin-top: 12px">
      <Spark values={store.history.cpu} max={100} />
    </div>

    {#if stats?.gpu}
      <div class="row" style="margin-top: 8px">
        <span class="grow">
          <div class="name">{stats.gpu.ime}</div>
          <div class="sub">{bytes(stats.gpu.mem_used)} / {bytes(stats.gpu.mem_total)} · {stats.gpu.temp}°C</div>
        </span>
        <span class="tag accent">{t('dashboard.gpu')} {percent(stats.gpu.zauzetost)}</span>
      </div>
    {:else}
      <div class="row">
        <span class="grow sub">{t('dashboard.gpu')}: —</span>
        <span class="tag">{t('dashboard.load')} {(stats?.cpu?.load?.[0] ?? 0).toFixed(2)}</span>
      </div>
    {/if}
    {#if stats?.process}
      <div class="row">
        <span class="grow sub">Rustiio (pid {stats.process.pid})</span>
        <span class="tag">{bytes(stats.process.memory)} · {percent(stats.process.cpu, 1)}</span>
      </div>
    {/if}
  </Card>

  <Card id="library" title={t('dashboard.library')} span={cardOf('library')?.span ?? 3} collapsed={cardOf('library')?.collapsed}>
    <div class="kpis">
      <div class="kpi"><b>{status?.items ?? 0}</b><span>{t('dashboard.items')}</span></div>
      <div class="kpi"><b>{counts.videos ?? 0}</b><span>{t('common.video')}</span></div>
      <div class="kpi"><b>{counts.folders ?? 0}</b><span>{t('common.folder')}</span></div>
    </div>
    <div class="row">
      <span class="grow sub">{t('dashboard.roots')}</span>
      <span class="tag">{status?.roots?.length ?? 0}</span>
    </div>
    <div class="row">
      <span class="grow sub">{t('dashboard.subscriptions')}</span>
      <span class="tag">{status?.subscriptions ?? 0}</span>
    </div>
    <div class="row">
      <span class="grow sub">Transcode</span>
      <span class="tag" class:ok={status?.transcode_enabled}>{status?.transcode_enabled ? t('dashboard.on') : t('dashboard.off')}</span>
    </div>
  </Card>

  <Card id="disks" title={t('dashboard.disks')} span={cardOf('disks')?.span ?? 4} collapsed={cardOf('disks')?.collapsed}>
    {#each stats?.disks ?? [] as disk (disk.montirano)}
      <div class="row">
        <span class="grow">
          <div class="name">{disk.ime} <span class="sub">{disk.montirano}</span></div>
          <div class="bar" style="margin-top: 6px">
            <i class={barClass(disk.posto)} style="width: {Math.min(100, disk.posto)}%"></i>
          </div>
        </span>
        <span class="tag">{percent(disk.posto)} · {bytes(disk.slobodno)} {t('dashboard.free')}</span>
      </div>
    {:else}
      <div class="empty">{t('common.none')}</div>
    {/each}
  </Card>

  <Card id="posters" title={t('dashboard.posters')} span={cardOf('posters')?.span ?? 4} collapsed={cardOf('posters')?.collapsed}>
    {#snippet actions()}
      <span class="tag" class:ok={posters?.tmdb_key}>{posters?.tmdb_key ? t('dashboard.tmdb_key') : t('dashboard.no_key')}</span>
      <button class="btn ghost" onclick={refreshPosters}>⟳</button>
    {/snippet}
    <div class="kpis">
      <div class="kpi"><b>{posters?.have ?? 0}</b><span>{t('dashboard.have_poster')}</span></div>
      <div class="kpi"><b>{posters?.pending ?? 0}</b><span>{t('dashboard.poster_pending')}</span></div>
      <div class="kpi"><b>{posters?.none ?? 0}</b><span>{t('dashboard.no_poster')}</span></div>
    </div>
    <div class="bar" style="margin-top: 10px">
      <i class={barClass(100 - posterPercent)} style="width: {posterPercent}%"></i>
    </div>
  </Card>

  <Card id="logs" title={t('dashboard.logs')} span={cardOf('logs')?.span ?? 4} collapsed={cardOf('logs')?.collapsed}>
    {#snippet actions()}
      <span class="tag" class:ok={store.logSocket === 'open'}>{store.logSocket === 'open' ? t('logs.connected') : t('logs.disconnected')}</span>
    {/snippet}
    <div class="logs" style="max-height: 240px">
      {#each store.logs.slice(-12) as line}
        <div class="log" class:error={line.level === 'ERROR'} class:warn={line.level === 'WARN'}>
          <span class="t">{line.at}</span>
          <span class="lvl {line.level}">{line.level}</span>
          <span class="msg">{line.message}</span>
        </div>
      {/each}
    </div>
  </Card>

  <div style="grid-column: 1 / -1; display: flex; justify-content: flex-end">
    <button class="btn ghost" onclick={resetLayout}>{t('common.reset')}</button>
  </div>
</div>
