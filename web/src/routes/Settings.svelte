<script>
  // Postavke: uređivanje configa iz browsera. Sve što traži restart se jasno kaže.
  import { t, i18n, setLocale, languages } from '../lib/i18n.svelte.js'
  import { get, put, post } from '../lib/api.js'
  import { toast, refreshStatus } from '../lib/store.svelte.js'

  let config = $state(null)
  let original = $state('')
  let path = $state('')
  let restartFields = $state([])
  let saving = $state(false)
  let rawOpen = $state(false)
  let raw = $state('')

  const dirty = $derived(config ? JSON.stringify(config) !== original : false)

  async function load() {
    try {
      const data = await get('/api/settings')
      config = data.config
      original = JSON.stringify(data.config)
      path = data.putanja
      restartFields = data.restart_prefiksi ?? []
      raw = JSON.stringify(data.config, null, 2)
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }

  async function save() {
    if (!config) return
    saving = true
    try {
      const result = await put('/api/settings', config)
      original = JSON.stringify(config)
      await refreshStatus()
      toast('ok', t('common.saved'))
      if (result?.restart_potreban) {
        toast('warn', `${t('settings.restart_needed')}: ${(result.promijenjena ?? []).join(', ')}`, 10000)
      }
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    } finally {
      saving = false
    }
  }

  function applyRaw() {
    try {
      config = JSON.parse(raw)
      toast('ok', t('common.saved'))
    } catch (error) {
      toast('err', `JSON: ${error.message}`)
    }
  }

  async function restart() {
    if (!confirm(t('settings.restart_confirm'))) return
    try {
      await post('/api/restart')
      toast('warn', t('settings.restarting'), 12000)
      setTimeout(() => location.reload(), 7000)
    } catch (error) {
      toast('err', `${t('common.error')}: ${error.message}`)
    }
  }

  function addRoot() {
    config.library.roots = [...(config.library.roots ?? []), { label: '', path: '/', kind: 'video' }]
  }

  function removeRoot(index) {
    config.library.roots = config.library.roots.filter((_, position) => position !== index)
  }

  $effect(() => {
    load()
  })
</script>

{#if !config}
  <div class="empty">{t('common.loading')}</div>
{:else}
  <div class="lib-bar">
    <span class="pill mono hide-sm">{path}</span>
    <span class="spacer"></span>
    {#if dirty}<span class="tag warn">{t('settings.restart_needed')}</span>{/if}
    <button class="btn primary" onclick={save} disabled={saving || !dirty}>{saving ? '…' : t('common.save')}</button>
    <button class="btn ghost" onclick={load} disabled={!dirty}>{t('common.cancel')}</button>
    <button class="btn danger" onclick={restart}>{t('settings.restart_now')}</button>
  </div>

  <div class="bento">
    <div class="card" style="--span: 6">
      <div class="card-head"><span class="card-title">{t('settings.server')}</span></div>
      <div class="card-body">
        <div class="form">
          <label for="s-name">{t('settings.friendly_name')}</label>
          <input id="s-name" class="field" bind:value={config.server.friendly_name} />

          <label for="s-bind">{t('settings.bind')}</label>
          <input id="s-bind" class="field" bind:value={config.server.bind} />

          <label for="s-port">{t('settings.port')}</label>
          <input id="s-port" class="field" type="number" bind:value={config.server.http_port} />

          <label for="s-ip">{t('settings.advertise_ip')}</label>
          <input id="s-ip" class="field" bind:value={config.server.advertise_ip} />

          <label for="s-udn">{t('settings.udn')}</label>
          <input id="s-udn" class="field mono" bind:value={config.server.udn} />

          <label for="s-log">log_level</label>
          <select id="s-log" class="field" bind:value={config.server.log_level}>
            {#each ['error', 'warn', 'info', 'debug'], level}
              <option value={level}>{level}</option>
            {/each}
          </select>

          <label for="s-ui">{t('common.language')}</label>
          <select
            id="s-ui"
            class="field"
            bind:value={config.ui.language}
            onchange={() => setLocale(config.ui.language)}
          >
            {#each languages as language}
              <option value={language.id}>{language.label()}</option>
            {/each}
          </select>
        </div>
      </div>
    </div>

    <div class="card" style="--span: 6">
      <div class="card-head">
        <span class="card-title">{t('settings.network')} · {t('settings.transcode')}</span>
      </div>
      <div class="card-body">
        <div class="form">
          <label for="n-ip">{t('settings.ip_family')}</label>
          <select id="n-ip" class="field" bind:value={config.network.ip_family}>
            {#each ['ipv4', 'ipv6', 'any'], family}<option value={family}>{family}</option>{/each}
          </select>

          <label for="n-timeout">timeout_connect</label>
          <input id="n-timeout" class="field" type="number" bind:value={config.network.timeout_connect} />

          <label for="t-on">{t('settings.enabled')}</label>
          <input id="t-on" type="checkbox" bind:checked={config.transcode.enabled} />

          <label for="t-hw">hw_accel</label>
          <input id="t-hw" class="field" bind:value={config.transcode.hw_accel} />

          <label for="t-max">{t('settings.max_concurrent')}</label>
          <input id="t-max" class="field" type="number" bind:value={config.transcode.max_concurrent} />

          <label for="t-ff">{t('settings.ffmpeg')}</label>
          <input id="t-ff" class="field mono" bind:value={config.transcode.ffmpeg_path} />

          <label for="t-fp">{t('settings.ffprobe')}</label>
          <input id="t-fp" class="field mono" bind:value={config.transcode.ffprobe_path} />
        </div>
      </div>
    </div>

    <div class="card" style="--span: 12">
      <div class="card-head">
        <span class="card-title">{t('settings.roots')}</span>
        <span class="card-actions">
          <button class="btn" onclick={addRoot}>+ {t('settings.add_root')}</button>
        </span>
      </div>
      <div class="card-body">
        {#each config.library.roots as root, index}
          <div class="row">
            <input class="field" style="flex: 0 1 150px" placeholder={t('settings.label')} bind:value={root.label} />
            <input class="field" style="flex: 1 1 320px" placeholder={t('settings.path')} bind:value={root.path} />
            <select class="field" bind:value={root.kind}>
              {#each ['video', 'audio', 'image'], kind}<option value={kind}>{kind}</option>{/each}
            </select>
            <button class="btn danger" onclick={() => removeRoot(index)}>✕</button>
          </div>
        {/each}
        <div class="form" style="margin-top: 14px">
          <label for="l-depth">max_depth</label>
          <input id="l-depth" class="field" type="number" bind:value={config.library.max_depth} />
          <label for="l-recent">recent_limit</label>
          <input id="l-recent" class="field" type="number" bind:value={config.library.recent_limit} />
          <label for="l-views">views</label>
          <input id="l-views" type="checkbox" bind:checked={config.library.views} />
        </div>
      </div>
    </div>

    <div class="card" style="--span: 12">
      <div class="card-head">
        <span class="card-title">JSON</span>
        <span class="card-actions">
          <button class="btn ghost" onclick={() => (rawOpen = !rawOpen)}>{rawOpen ? '▾' : '▸'}</button>
        </span>
      </div>
      {#if rawOpen}
        <div class="card-body">
          <textarea
            class="field mono"
            style="width: 100%; min-height: 260px"
            bind:value={raw}
            spellcheck="false"
          ></textarea>
          <div style="margin-top: 8px; display: flex; gap: 8px">
            <button class="btn" onclick={applyRaw}>{t('common.save')}</button>
            <button class="btn ghost" onclick={() => (raw = JSON.stringify(config, null, 2))}>{t('common.reset')}</button>
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}
