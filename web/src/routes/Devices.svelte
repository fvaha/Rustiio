<script>
  // Uređaji: tko se javio, što je tražio i koji profil je dobio.
  // Iz zapisa se može napraviti pravi profil (TOML) za taj uređaj.
  import { t } from '../lib/i18n.svelte.js'
  import { store, refreshProfiles, refreshDevices, toast } from '../lib/store.svelte.js'
  import { get, post } from '../lib/api.js'
  import { ago, dateTime } from '../lib/format.js'

  let opened = $state('')
  let generated = $state({})

  const devices = $derived(store.devices?.devices ?? [])
  const profiles = $derived(store.profiles?.profiles ?? [])

  async function showToml(device) {
    if (opened === device.key) {
      opened = ''
      return
    }
    try {
      const text = await get(`/api/profile/${device.key}`, { headers: { accept: 'text/plain' } })
      generated = { ...generated, [device.key]: text }
      opened = device.key
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }

  /// Ime profila iz User-Agenta: `Samsung UE55` → `samsung-ue55`.
  function suggestId(device) {
    const source = device.friendly_name || device.user_agent || device.key
    return source
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '')
      .slice(0, 40)
  }

  async function saveProfile(device) {
    const id = suggestId(device)
    try {
      const result = await post(`/api/profile/${device.key}`, { id })
      toast('ok', `Profil spremljen: ${result?.id ?? id}`)
      await Promise.all([refreshProfiles(), refreshDevices()])
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }
</script>

<div class="bento">
  <div class="card" style="--span: 7">
    <div class="card-head"><span class="card-title">{t('devices.title')}</span></div>
    <div class="card-body">
      {#if devices.length === 0}
        <div class="empty">{t('devices.none')}</div>
      {:else}
        <table class="grid">
          <thead>
            <tr>
              <th>{t('devices.name')}</th>
              <th>{t('devices.profile')}</th>
              <th>{t('devices.seen')}</th>
              <th class="right">{t('devices.requests')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each devices as device (device.key)}
              <tr>
                <td>
                  <div class="name">{device.friendly_name || device.user_agent || device.key}</div>
                  <div class="sub muted mono hide-sm">{device.ip} · {device.user_agent || '—'}</div>
                </td>
                <td><span class="tag accent">{device.profile || '—'}</span></td>
                <td class="muted" title={dateTime(device.last_seen)}>{ago(device.last_seen)}</td>
                <td class="right">{device.requests}</td>
                <td class="right" style="white-space: nowrap">
                  <button class="btn ghost" onclick={() => showToml(device)}>{opened === device.key ? '▾' : '▸'} TOML</button>
                  <button class="btn" onclick={() => saveProfile(device)}>{t('common.save')}</button>
                </td>
              </tr>
              {#if opened === device.key}
                <tr>
                  <td colspan="5">
                    <div class="muted small" style="margin-bottom: 6px">{t('devices.headers')}:</div>
                    <div class="mono small" style="margin-bottom: 8px">
                      {#each Object.entries(device.headers ?? {}) as [key, value]}
                        <div>{key}: {value}</div>
                      {:else}
                        <span class="muted">—</span>
                      {/each}
                    </div>
                    <pre class="mono small" style="overflow-x: auto; background: #0d1014; padding: 10px; border-radius: 10px">
{generated[device.key] ?? t('common.loading')}</pre
                    >
                  </td>
                </tr>
              {/if}
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  </div>

  <div class="card" style="--span: 5">
    <div class="card-head">
      <span class="card-title">{t('devices.profiles_title')}</span>
      <span class="card-actions">
        <span class="pill">{store.profiles?.count ?? 0}</span>
      </span>
    </div>
    <div class="card-body">
      {#if profiles.length === 0}
        <div class="empty">{t('common.none')}</div>
      {:else}
        {#each profiles as profile (profile.id)}
          <div class="row">
            <span class="grow">
              <div class="name">{profile.name || profile.id}</div>
              <div class="sub">
                {profile.target || t('devices.generic')}
                {#if profile.max_height} · {profile.max_height}p{/if}
                {#if profile.video_codecs?.length} · {profile.video_codecs.slice(0, 3).join('/')}{/if}
                {#if profile.subtitle_mode} · {profile.subtitle_mode}{/if}
              </div>
            </span>
            <span class="tag" class:ok={profile.rules?.length}>{profile.rules?.length ?? 0} {t('library.rules')}</span>
          </div>
        {/each}
      {/if}
      <div class="muted small" style="margin-top: 10px">{store.profiles?.dir}</div>
    </div>
  </div>
</div>
