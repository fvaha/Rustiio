<script>
  // Uređaji: tko se javio, što je tražio i koji profil je dobio.
  // Iz zapisa se može napraviti pravi profil (TOML) za taj uređaj.
  import { onMount } from 'svelte'
  import { t } from '../lib/i18n.svelte.js'
  import { store, refreshProfiles, refreshDevices, toast } from '../lib/store.svelte.js'
  import { get, post, put } from '../lib/api.js'
  import { ago, dateTime } from '../lib/format.js'

  let opened = $state('')
  let generated = $state({})

  const devices = $derived(store.devices?.devices ?? [])
  const profiles = $derived(store.profiles?.profiles ?? [])

  onMount(() => {
    // Profili se prije nisu dohvaćali na ovoj stranici, pa je desna kartica
    // uvijek pisala „nema podataka" iako ih server ima 17.
    if (!store.profiles) refreshProfiles()
  })

  /// Prepoznatljivo ime iz User-Agenta — sirovi UA se lomi u nekoliko redaka i
  /// razvali tablicu, pa ide u opis s točkicama (cijeli je u `title`).
  function label(device) {
    if (device.friendly_name) return device.friendly_name
    const ua = device.user_agent ?? ''
    const uzorci = [
      [/Portable SDK for UPnP devices\/([\d.]+)/i, (m) => `UPnP SDK ${m[1]}`],
      [/VLC\/([\d.]+)/i, (m) => `VLC ${m[1]}`],
      [/LibVLC\/([\d.]+)/i, (m) => `LibVLC ${m[1]}`],
      [/Kodi\/([\d.]+)/i, (m) => `Kodi ${m[1]}`],
      [/Windows-Media-Player\/[\d.]+/i, () => 'Windows Media Player'],
      [/Samsung[^\s);,]*/i, (m) => m[0]],
      [/DLNADOC\/[\d.]+/i, () => t('devices.dlna_client')],
    ].find(([uzorak]) => uzorak.test(ua))
    if (uzorci) {
      const [uzorak, pretvori] = uzorci
      const pogodak = ua.match(uzorak)
      if (pogodak) return pretvori(pogodak)
    }
    return device.key || device.ip || '?'
  }

  /// Obitelj uređaja iz User-Agenta (mala oznaka uz ime).
  function platform(device) {
    const ua = device.user_agent ?? ''
    if (/android/i.test(ua)) return 'Android'
    if (/iphone|ipad|ios/i.test(ua)) return 'iOS'
    if (/windows/i.test(ua)) return 'Windows'
    if (/mac os|darwin/i.test(ua)) return 'macOS'
    if (/linux|upnp/i.test(ua)) return 'Linux'
    return ''
  }

  /// Koliko se pravila poklapa (u API-ju je `rules` objekt s tri popisa).
  function ruleCount(profile) {
    const rules = profile.rules ?? {}
    return Object.values(rules).reduce((zbroj, popis) => zbroj + (Array.isArray(popis) ? popis.length : 0), 0)
  }

  /// Kratki sažetak profila: rezolucija, kontejneri, titlovi.
  function profileSummary(profile) {
    const dijelovi = []
    if (profile.max_height) dijelovi.push(`${profile.max_height}p`)
    if (profile.containers?.length) dijelovi.push(profile.containers.slice(0, 4).join('/'))
    if (profile.video_codecs?.length) dijelovi.push(profile.video_codecs.slice(0, 2).join('/'))
    if (profile.subtitle_mode) dijelovi.push(profile.subtitle_mode)
    return dijelovi.join(' · ')
  }

  async function showToml(device) {
    if (opened === device.key) {
      opened = ''
      return
    }
    try {
      // Ključ uređaja zna sadržavati `/` i razmake (`ua:VLC/3.0.23 LibVLC/3.0.23`)
      // pa mora u URL enkodiran — inače zahtjev ode na pogrešnu rutu (405).
      const text = await get(`/api/profile/${encodeURIComponent(device.key)}`, { headers: { accept: 'text/plain' } })
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

  // ── Pravila profila: dubina boje i ostalo se mijenja po uređaju ──────────
  let editId = $state('')
  let pravila = $state(null)

  /// Učitaj pravila profila u obrazac (popisi kao tekst odvojen zarezom).
  function ucitajPravila(id) {
    editId = id
    const profil = profiles.find((item) => item.id === id)
    if (!profil) {
      pravila = null
      return
    }
    pravila = {
      containers: (profil.containers ?? []).join(', '),
      codecs: (profil.video_codecs ?? []).join(', '),
      max_width: profil.max_width ?? 0,
      max_height: profil.max_height ?? 0,
      max_bit_depth: profil.max_bit_depth ?? 8,
      max_bitrate_kbps: profil.max_bitrate_kbps ?? 0,
      audio_codecs: (profil.audio_codecs ?? []).join(', '),
      max_channels: profil.max_channels ?? 2,
      target_container: profil.target?.container ?? '',
      target_video: profil.target?.video_codec ?? '',
      target_audio: profil.target?.audio_codec ?? '',
      target_bitrate: profil.target?.max_bitrate_kbps ?? 0,
    }
  }

  async function spremiPravila() {
    if (!editId || !pravila) return
    const popis = (tekst) =>
      String(tekst ?? '')
        .split(',')
        .map((dio) => dio.trim())
        .filter(Boolean)
    try {
      await put(`/api/profiles/${encodeURIComponent(editId)}`, {
        video: {
          containers: popis(pravila.containers),
          codecs: popis(pravila.codecs),
          max_width: Number(pravila.max_width) || 0,
          max_height: Number(pravila.max_height) || 0,
          max_bit_depth: Number(pravila.max_bit_depth) || 8,
          max_bitrate_kbps: Number(pravila.max_bitrate_kbps) || 0,
        },
        audio: {
          codecs: popis(pravila.audio_codecs),
          max_channels: Number(pravila.max_channels) || 2,
        },
        transcode: {
          container: pravila.target_container,
          video_codec: pravila.target_video,
          audio_codec: pravila.target_audio,
          max_bitrate_kbps: Number(pravila.target_bitrate) || 0,
        },
      })
      toast('ok', t('devices.saved_rules'))
      await refreshProfiles()
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }

  async function vratiUgradeno() {
    if (!editId) return
    const prije = editId
    try {
      await post(`/api/profiles/${encodeURIComponent(prije)}/reset`, {})
      toast('ok', t('devices.reset_done'))
      await refreshProfiles()
      ucitajPravila(prije)
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }

  async function dodijeliProfil(device, profileId) {
    try {
      await put('/api/device-profile', { key: device.key, profile_id: profileId })
      toast('ok', `${t('devices.profile')}: ${profileId || t('devices.profile_auto')}`)
      await refreshDevices()
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }

  async function saveProfile(device) {
    const id = suggestId(device)
    try {
      const result = await post(`/api/profile/${encodeURIComponent(device.key)}`, { id })
      toast('ok', `${t('devices.saved')}: ${result?.id ?? id}`)
      await Promise.all([refreshProfiles(), refreshDevices()])
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }
</script>

<div class="bento">
  <div class="card" style="--span: 7">
    <div class="card-head">
      <span class="card-title" title={t('devices.hint')}>{t('devices.title')}</span>
      <span class="muted small hide-narrow">{t('devices.hint')}</span>
      <span class="card-actions">
        <span class="pill">{devices.length}</span>
        <button class="btn ghost" onclick={() => refreshDevices()} title={t('common.refresh')}>⟳</button>
      </span>
    </div>
    <div class="card-body">
      {#if devices.length === 0}
        <div class="empty">{t('devices.none')}</div>
      {:else}
        <div class="scroll">
          <table class="tbl">
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
                    <div class="dev">
                      <span class="name" title={device.user_agent || device.key}>{label(device)}</span>
                      {#if platform(device)}<span class="tag">{platform(device)}</span>{/if}
                    </div>
                    <div class="sub mono cut" title={device.user_agent ?? ''}>
                      {device.ip}{device.user_agent ? ` · ${device.user_agent}` : ''}
                    </div>
                  </td>
                  <td>
                    <div class="prof">
                      <span class="tag accent">{device.profile || '—'}</span>
                      {#if device.profile_choice}<span class="tag ok" title={t('devices.chosen')}>{t('devices.chosen')}</span>{/if}
                    </div>
                  </td>
                  <td class="muted nowrap" title={dateTime(device.last_seen)}>{ago(device.last_seen)}</td>
                  <td class="right">{device.requests}</td>
                  <td class="right nowrap">
                    <select
                      class="select sm assign"
                      title={t('devices.assign_hint')}
                      value={device.profile_choice ?? ''}
                      onchange={(event) => dodijeliProfil(device, event.currentTarget.value)}
                    >
                      <option value="">{t('devices.profile_auto')}</option>
                      {#each profiles as profile (profile.id)}
                        <option value={profile.id}>{profile.id}</option>
                      {/each}
                    </select>
                    <button class="btn ghost" onclick={() => showToml(device)} title={t('devices.toml_hint')}>
                      {opened === device.key ? '▾' : '▸'} TOML
                    </button>
                    <button class="btn" onclick={() => saveProfile(device)} title={t('devices.make_profile_hint')}>
                      {t('devices.make_profile')}
                    </button>
                  </td>
                </tr>
                {#if opened === device.key}
                  <tr>
                    <td colspan="5">
                      <div class="muted small" style="margin-bottom: 6px">{t('devices.headers')}:</div>
                      <div class="mono small" style="margin-bottom: 8px">
                        {#each Object.entries(device.headers ?? {}) as [key, value]}
                          <div class="cut" title="{key}: {value}">{key}: {value}</div>
                        {:else}
                          <span class="muted">—</span>
                        {/each}
                      </div>
                      <pre class="mono small">{generated[device.key] ?? t('common.loading')}</pre>
                    </td>
                  </tr>
                {/if}
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  </div>

  <div class="card" style="--span: 5">
    <div class="card-head">
      <span class="card-title">{t('devices.profiles_title')}</span>
      <span class="card-actions"><span class="pill">{profiles.length}</span></span>
    </div>
    <div class="card-body">
      {#if profiles.length === 0}
        <div class="empty">{t('common.none')}</div>
      {:else}
        <div class="scroll tall">
          {#each profiles as profile (profile.id)}
            <div class="row tight">
              <span class="grow">
                <div class="prof">
                  <span class="name" title={profile.description || profile.id}>{profile.name || profile.id}</span>
                  <span class="tag mono">{profile.id}</span>
                </div>
                <div class="sub cut" title={profileSummary(profile)}>{profileSummary(profile)}</div>
              </span>
              <span class="tag" class:ok={ruleCount(profile) > 0} title={t('devices.rules_hint')}>
                {ruleCount(profile)} {t('library.rules')}
              </span>
            </div>
          {/each}
        </div>
      {/if}
      <div class="muted small mono cut" style="margin-top: 10px" title={store.profiles?.dir}>{store.profiles?.dir}</div>
    </div>
  </div>

  <div class="card" style="--span: 12">
    <div class="card-head">
      <span class="card-title">{t('devices.rules_title')}</span>
      <span class="muted small hide-narrow">{t('devices.rules_note')}</span>
      <span class="card-actions">
        <select
          class="select"
          title={t('devices.pick_profile')}
          value={editId}
          onchange={(event) => ucitajPravila(event.currentTarget.value)}
        >
          <option value="">{t('devices.pick_profile')}</option>
          {#each profiles as profile (profile.id)}
            <option value={profile.id}>{profile.id}</option>
          {/each}
        </select>
      </span>
    </div>
    <div class="card-body">
      {#if !pravila}
        <div class="empty">{t('devices.pick_profile')}</div>
      {:else}
        <div class="pravila">
          <label class="polje">
            <span>{t('devices.video_codecs')}</span>
            <input class="input" bind:value={pravila.codecs} title={t('devices.list_hint')} />
          </label>
          <label class="polje">
            <span>{t('devices.containers')}</span>
            <input class="input" bind:value={pravila.containers} title={t('devices.list_hint')} />
          </label>
          <label class="polje">
            <span>{t('devices.max_height')}</span>
            <input class="input num" type="number" bind:value={pravila.max_height} />
          </label>
          <label class="polje bit">
            <span>{t('devices.bit_depth')}</span>
            <input class="input num" type="number" min="8" max="16" bind:value={pravila.max_bit_depth} />
          </label>
          <label class="polje">
            <span>{t('devices.max_bitrate')}</span>
            <input class="input num" type="number" bind:value={pravila.max_bitrate_kbps} />
          </label>
          <label class="polje">
            <span>{t('devices.audio_codecs')}</span>
            <input class="input" bind:value={pravila.audio_codecs} title={t('devices.list_hint')} />
          </label>
          <label class="polje">
            <span>{t('devices.max_channels')}</span>
            <input class="input num" type="number" min="1" max="8" bind:value={pravila.max_channels} />
          </label>
        </div>

        <div class="podnaslov">{t('devices.target')} <span class="muted">· {t('devices.keep_small')}</span></div>
        <div class="pravila">
          <label class="polje">
            <span>{t('devices.target_video')}</span>
            <input class="input" bind:value={pravila.target_video} placeholder="h264" />
          </label>
          <label class="polje">
            <span>{t('devices.target_audio')}</span>
            <input class="input" bind:value={pravila.target_audio} placeholder="aac" />
          </label>
          <label class="polje">
            <span>{t('devices.target_container')}</span>
            <input class="input" bind:value={pravila.target_container} placeholder="ts" />
          </label>
          <label class="polje">
            <span>{t('devices.target_bitrate')}</span>
            <input class="input num" type="number" bind:value={pravila.target_bitrate} />
          </label>
        </div>

        <div class="akcije">
          <button class="btn" onclick={spremiPravila}>{t('devices.save_rules')}</button>
          <button class="btn ghost" onclick={vratiUgradeno}>{t('devices.reset_rules')}</button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  /* Tablica unutar kartice ne smije rasti u nedogled — pomiče se sama. */
  .scroll {
    max-height: 460px;
    overflow: auto;
  }
  .scroll.tall {
    max-height: 520px;
  }
  /* Zaglavlje ostaje na mjestu dok se redovi pomiču. */
  .tbl thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface);
    white-space: nowrap;
  }
  .tbl td {
    vertical-align: middle;
  }
  .dev {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .dev .name {
    font-weight: 600;
  }
  .prof {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  /* Jedan redak, s točkicama: sirovi User-Agent je dug i prije je lomio redak
     u pet redova (cijeli tekst je u `title`). */
  .cut {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 46ch;
  }
  .nowrap {
    white-space: nowrap;
  }
  /* Objašnjenje u zaglavlju ne stane na uski ekran. */
  @media (max-width: 820px) {
    .hide-narrow {
      display: none;
    }
  }
  /* Pravila profila: mreža polja koja se na telefonu sama slažu u jednu kolonu. */
  .pravila {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: 10px 14px;
  }
  .pravila .polje {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .pravila .polje > span {
    font-size: 12px;
    color: var(--muted-foreground);
  }
  /* Dubina boje je najvažnija za 10-bit — malo istaknuta. */
  .pravila .polje.bit .input {
    border-color: var(--accent);
  }
  .podnaslov {
    margin: 16px 0 8px;
    font-size: 12.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .akcije {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 14px;
  }
  .assign {
    max-width: 190px;
  }
  pre {
    overflow-x: auto;
    background: #0d1014;
    padding: 10px;
    border-radius: 10px;
    margin: 0;
  }
</style>
