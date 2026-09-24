<script>
  // Postavke: **sva** polja configa, po sekcijama, s objašnjenjem i spremljenim stanjem.
  // Polja su opisana u lib/settings-schema.js — novo polje u configu se pojavi samo.
  import { onMount } from 'svelte'
  import { i18n } from '../lib/i18n.svelte.js'
  import { get, put, post } from '../lib/api.js'
  import { toast, refreshStatus } from '../lib/store.svelte.js'
  import { SECTIONS, fieldsOf, getPath, setPath, metaOf } from '../lib/settings-schema.js'
  import Field from '../components/Field.svelte'

  let config = $state(null)
  let original = $state(null)
  let path = $state('')
  let pendingRestart = $state([])
  let saving = $state(false)
  let problem = $state('')
  let rawOpen = $state(false)
  let rawText = $state('')
  let rawError = $state('')

  const hr = $derived(i18n.lang !== 'en')
  const text = (item) => (item && typeof item === 'object' ? (item[hr ? 'hr' : 'en'] ?? item.hr ?? '') : (item ?? ''))

  /// Sva polja koja se razlikuju od spremljenog stanja (ista logika kao na serveru).
  const changed = $derived.by(() => diff(original, config))
  const restartFields = $derived(changed.filter((item) => !item.startsWith('ui.')))

  function diff(oldValue, newValue, prefix = '', out = []) {
    if (oldValue === newValue) return out
    const bothObjects = oldValue && newValue && typeof oldValue === 'object' && typeof newValue === 'object'
    if (!bothObjects) {
      if (prefix) out.push(prefix)
      return out
    }
    if (Array.isArray(oldValue) !== Array.isArray(newValue)) {
      out.push(prefix)
      return out
    }
    if (Array.isArray(oldValue)) {
      if (oldValue.length !== newValue.length) {
        out.push(prefix)
        return out
      }
      oldValue.forEach((item, index) => diff(item, newValue[index], `${prefix}[${index}]`, out))
      return out
    }
    const keys = new Set([...Object.keys(oldValue), ...Object.keys(newValue)])
    for (const key of keys) {
      diff(oldValue[key], newValue[key], prefix ? `${prefix}.${key}` : key, out)
    }
    return out
  }

  async function load() {
    try {
      const data = await get('/api/settings')
      config = data.config
      original = JSON.parse(JSON.stringify(data.config))
      path = data.putanja
      pendingRestart = data.ceka_restart ?? []
      rawText = JSON.stringify(data.config, null, 2)
      problem = ''
    } catch (error) {
      problem = `${hr ? 'Ne mogu pročitati postavke' : 'Cannot read settings'}: ${error.message}`
      toast('err', problem)
    }
  }

  onMount(load)

  function change(fieldPath, value) {
    config = setPath(config, fieldPath, value)
    rawText = JSON.stringify(config, null, 2)
  }

  function reset() {
    config = JSON.parse(JSON.stringify(original))
    rawText = JSON.stringify(config, null, 2)
    problem = ''
  }

  async function save() {
    saving = true
    problem = ''
    try {
      const result = await put('/api/settings', config)
      original = JSON.parse(JSON.stringify(config))
      pendingRestart = result?.traze_restart ?? []
      const count = result?.promijenjena?.length ?? 0
      toast('ok', hr ? `Spremljeno (${count} ${count === 1 ? 'polje' : 'polja'})` : `Saved (${count} fields)`)
      if (result?.restart_potreban) {
        toast('warn', hr ? 'Neka polja vrijede tek nakon restarta servisa.' : 'Some fields apply only after a service restart.', 9000)
      }
      rawText = JSON.stringify(config, null, 2)
      await refreshStatus()
    } catch (error) {
      problem = error.message
      toast('err', `${hr ? 'Nije spremljeno' : 'Not saved'}: ${error.message}`, 12000)
    } finally {
      saving = false
    }
  }

  async function restartService() {
    if (!confirm(hr ? 'Restartati servis sada? Strimovi će se prekinuti.' : 'Restart the service now? Streams will drop.')) return
    try {
      await post('/api/restart')
      toast('warn', hr ? 'Servis se diže… stranica će se osvježiti.' : 'Service is coming back… page will reload.', 8000)
      setTimeout(() => location.reload(), 6000)
    } catch (error) {
      toast('err', error.message)
    }
  }

  function applyRaw() {
    try {
      const parsed = JSON.parse(rawText)
      config = parsed
      rawError = ''
      toast('ok', hr ? 'Config pročitan iz JSON-a — provjeri i spremi.' : 'Config parsed from JSON — review and save.')
    } catch (error) {
      rawError = error.message
      toast('err', `${hr ? 'JSON nije ispravan' : 'Invalid JSON'}: ${error.message}`)
    }
  }

  const counts = $derived(
    config ? Object.fromEntries(SECTIONS.map((section) => [section.key, fieldsOf(section.key, config).length])) : {},
  )
</script>

<div class="settings">
  <nav class="settings-nav" aria-label={hr ? 'Sekcije postavki' : 'Settings sections'}>
    {#each SECTIONS as section}
      <a href="#{section.key}">
        <span>{text(section.title)}</span>
        <span class="n">{counts[section.key] ?? 0}</span>
      </a>
    {/each}
    <a href="#napredno"><span>{hr ? 'Napredno' : 'Advanced'}</span><span class="n">JSON</span></a>
  </nav>

  <div>
    {#if problem}
      <div class="panel" style="border-color: rgba(251, 113, 133, 0.5); margin-bottom: 16px">
        <div class="panel-body" style="padding-top: 14px">
          <strong style="color: var(--err)">{hr ? 'Greška' : 'Error'}:</strong> {problem}
        </div>
      </div>
    {/if}

    {#if pendingRestart.length}
      <div class="panel" style="border-color: rgba(251, 191, 36, 0.45); margin-bottom: 16px">
        <div class="panel-body" style="padding-top: 14px; display: flex; gap: 12px; align-items: center; flex-wrap: wrap">
          <span class="badge warn"><span class="dot"></span>{hr ? 'čeka restart' : 'restart pending'}</span>
          <span class="grow small dim">
            {hr ? 'Ova polja su spremljena, ali vrijede tek kad se servis digne:' : 'These fields are saved but take effect only after a restart:'}
            <span class="mono">{pendingRestart.join(', ')}</span>
          </span>
          <button class="btn primary" onclick={restartService}>{hr ? 'Restartaj servis' : 'Restart service'}</button>
        </div>
      </div>
    {/if}

    {#if !config}
      <div class="panel"><div class="panel-body">
        <div class="skeleton" style="height: 88px"></div>
        <div class="skeleton" style="height: 188px; margin-top: 12px"></div>
      </div></div>
    {:else}
      {#each SECTIONS as section}
        <section class="panel" id={section.key}>
          <div class="panel-head">
            <h2><span aria-hidden="true">{section.icon}</span> {text(section.title)}</h2>
            <p>{text(section.desc)}</p>
          </div>
          <div class="panel-body">
            {#each fieldsOf(section.key, config) as field (field.path)}
              <Field
                path={field.path}
                meta={metaOf(field.path, field.value)}
                value={field.value}
                changed={changed.some((item) => item === field.path || item.startsWith(`${field.path}[`))}
                onchange={(next) => change(field.path, next)}
              />
            {/each}
          </div>
        </section>
      {/each}

      <section class="panel" id="napredno">
        <div class="panel-head">
          <h2><span aria-hidden="true">⌘</span> {hr ? 'Napredno' : 'Advanced'}</h2>
          <p>
            {hr ? 'Cijeli config kao JSON. Koristi kad nešto nije u obrascu — i za kopiranje postavki na drugi server.' : 'The whole config as JSON. Use it for anything not in the form — and to copy settings to another server.'}
          </p>
        </div>
        <div class="panel-body">
          <div class="row tight">
            <span class="grow small faint mono">{path}</span>
            <button class="btn sm" onclick={() => (rawOpen = !rawOpen)}>{rawOpen ? (hr ? 'Sakrij' : 'Hide') : (hr ? 'Prikaži' : 'Show')}</button>
            <button class="btn sm" onclick={applyRaw} disabled={!rawOpen}>{hr ? 'Primijeni' : 'Apply'}</button>
          </div>
          {#if rawOpen}
            <textarea class="textarea mono" rows="18" bind:value={rawText} spellcheck="false"></textarea>
            {#if rawError}<div class="tiny" style="color: var(--err); margin-top: 6px">{rawError}</div>{/if}
          {/if}
        </div>
      </section>

      <div class="savebar">
        <span class="badge" class:info={changed.length > 0} class:ok={changed.length === 0}>
          {changed.length > 0
            ? `${changed.length} ${hr ? (changed.length === 1 ? 'promjena' : 'promjena') : 'changed'}`
            : hr ? 'sve spremljeno' : 'all saved'}
        </span>
        {#if changed.length > 0}
          <span class="small faint mono hide-sm" style="max-width: 46ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">
            {changed.join(', ')}
          </span>
        {/if}
        <span class="spacer"></span>
        {#if restartFields.length > 0 && changed.length === 0}
          <span class="badge warn">{hr ? 'restart čeka' : 'restart pending'}</span>
        {/if}
        <button class="btn" onclick={reset} disabled={changed.length === 0}>{hr ? 'Vrati' : 'Revert'}</button>
        <button class="btn primary" onclick={save} disabled={saving || changed.length === 0}>
          {saving ? (hr ? 'Spremam…' : 'Saving…') : (hr ? 'Spremi postavke' : 'Save settings')}
        </button>
      </div>
    {/if}
  </div>
</div>
