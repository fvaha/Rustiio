<script>
  // Postavke: **sva** polja configa, po sekcijama, s objašnjenjem i spremljenim stanjem.
  // Sekcije su **tabovi iznad** — klik otvara samo tu sekciju, ne jedna duga strana.
  // Polja su opisana u lib/settings-schema.js — novo polje u configu se pojavi samo.
  import { onMount } from 'svelte'
  import { t, i18n } from '../lib/i18n.svelte.js'
  import { get, put, post } from '../lib/api.js'
  import { toast, refreshStatus } from '../lib/store.svelte.js'
  import { SECTIONS, fieldsOf, getPath, setPath, metaOf } from '../lib/settings-schema.js'
  import Field from '../components/Field.svelte'
  import TranscodeScan from '../components/TranscodeScan.svelte'

  let config = $state(null)
  let original = $state(null)
  let path = $state('')
  let pendingRestart = $state([])
  let saving = $state(false)
  let problem = $state('')
  let rawOpen = $state(false)
  let rawText = $state('')
  let rawError = $state('')

  // Aktivni tab. Adresa ostaje `#/settings` (sekcija se NE stavlja u adresu — router
  // bi je čitao kao stranicu i vraćao na Pregled).
  let tab = $state(sessionStorage.getItem('rustiio:settings-tab') ?? 'server')
  $effect(() => sessionStorage.setItem('rustiio:settings-tab', tab))

  const hr = $derived(i18n.lang !== 'en')
  const text = (item) => (item && typeof item === 'object' ? (item[t('field.en')] ?? item.hr ?? '') : (item ?? ''))

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
      problem = `${t('set.cannot_read_settings')}: ${error.message}`
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
        toast('warn', t('set.some_fields_apply_only_after_a_service_res'), 9000)
      }
      rawText = JSON.stringify(config, null, 2)
      await refreshStatus()
    } catch (error) {
      problem = error.message
      toast('err', `${t('set.not_saved')}: ${error.message}`, 12000)
    } finally {
      saving = false
    }
  }

  async function restartService() {
    if (!confirm(t('set.restart_the_service_now_streams_will_drop'))) return
    try {
      await post('/api/restart')
      toast('warn', t('set.service_is_coming_back_page_will_reload'), 8000)
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
      toast('ok', t('set.config_parsed_from_json_review_and_save'))
    } catch (error) {
      rawError = error.message
      toast('err', `${t('set.invalid_json')}: ${error.message}`)
    }
  }

  const aktivna = $derived(SECTIONS.find((section) => section.key === tab) ?? null)
  const counts = $derived(
    config ? Object.fromEntries(SECTIONS.map((section) => [section.key, fieldsOf(section.key, config).length])) : {},
  )
  // Koliko polja po sekciji čeka restart — tab to pokaže sitnom točkom.
  const ceka = $derived(
    config
      ? Object.fromEntries(
          SECTIONS.map((section) => [
            section.key,
            fieldsOf(section.key, config).some((field) => changed.includes(field.path) && !field.path.startsWith('ui.')),
          ]),
        )
      : {},
  )
</script>

<div class="settings">
  <nav class="tabs" aria-label={t('set.settings_sections')} role="tablist">
    {#each SECTIONS as section}
      <button
        type="button"
        role="tab"
        class="tab"
        class:active={tab === section.key}
        aria-selected={tab === section.key}
        onclick={() => (tab = section.key)}
      >
        <span>{text(section.title)}</span>
        <span class="n">{counts[section.key] ?? 0}</span>
        {#if ceka[section.key]}<span class="dot" title={t('set.restart_pending')}></span>{/if}
      </button>
    {/each}
    <button
      type="button"
      role="tab"
      class="tab"
      class:active={tab === 'napredno'}
      aria-selected={tab === 'napredno'}
      onclick={() => (tab = 'napredno')}
    >
      <span>{t('set.advanced')}</span>
      <span class="n">JSON</span>
    </button>
  </nav>

  <div class="sadrzaj">
    {#if problem}
      <div class="panel" style="border-color: rgba(251, 113, 133, 0.5); margin-bottom: 16px">
        <div class="panel-body" style="padding-top: 14px">
          <strong style="color: var(--err)">{t('set.error')}:</strong> {problem}
        </div>
      </div>
    {/if}

    {#if pendingRestart.length}
      <div class="panel" style="border-color: rgba(251, 191, 36, 0.45); margin-bottom: 16px">
        <div class="panel-body" style="padding-top: 14px; display: flex; gap: 12px; align-items: center; flex-wrap: wrap">
          <span class="badge warn"><span class="dot"></span>{t('set.restart_pending')}</span>
          <span class="grow small dim">
            {t('set.these_fields_are_saved_but_take_effect_onl')}
            <span class="mono">{pendingRestart.join(', ')}</span>
          </span>
          <button class="btn primary" onclick={restartService}>{t('set.restart_service')}</button>
        </div>
      </div>
    {/if}

    {#if !config}
      <div class="panel"><div class="panel-body">
        <div class="skeleton" style="height: 88px"></div>
        <div class="skeleton" style="height: 188px; margin-top: 12px"></div>
      </div></div>
    {:else if tab === 'napredno'}
      <section class="panel" id="napredno">
        <div class="panel-head">
          <h2><span aria-hidden="true">⌘</span> {t('set.advanced')}</h2>
          <p>
            {t('set.the_whole_config_as_json_use_it_for_anythi')}
          </p>
        </div>
        <div class="panel-body">
          <div class="row tight">
            <span class="grow small faint mono">{path}</span>
            <button class="btn sm" onclick={() => (rawOpen = !rawOpen)}>{rawOpen ? (t('set.hide')) : (t('set.show'))}</button>
            <button class="btn sm" onclick={applyRaw} disabled={!rawOpen}>{t('set.apply')}</button>
          </div>
          {#if rawOpen}
            <textarea class="textarea mono" rows="18" bind:value={rawText} spellcheck="false"></textarea>
            {#if rawError}<div class="tiny" style="color: var(--err); margin-top: 6px">{rawError}</div>{/if}
          {/if}
        </div>
      </section>
    {:else if aktivna}
      <section class="panel" id={aktivna.key}>
        <div class="panel-head">
          <h2> {text(aktivna.title)}</h2>
          <p>{text(aktivna.desc)}</p>
        </div>
        <div class="panel-body">
          {#each fieldsOf(aktivna.key, config) as field (field.path)}
            <Field
              path={field.path}
              meta={metaOf(field.path, field.value)}
              value={field.value}
              changed={changed.some((item) => item === field.path || item.startsWith(`${field.path}[`))}
              onchange={(next) => change(field.path, next)}
            />
          {/each}
          {#if aktivna.key === 'transcode'}
            <TranscodeScan />
          {/if}
        </div>
      </section>
    {/if}

    {#if config}
      <div class="savebar">
        <span class="badge" class:info={changed.length > 0} class:ok={changed.length === 0}>
          {changed.length > 0
            ? `${changed.length} ${hr ? (changed.length === 1 ? 'promjena' : 'promjena') : 'changed'}`
            : t('set.all_saved')}
        </span>
        {#if changed.length > 0}
          <span class="small faint mono hide-sm" style="max-width: 46ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">
            {changed.join(', ')}
          </span>
        {/if}
        <span class="spacer"></span>
        {#if restartFields.length > 0 && changed.length === 0}
          <span class="badge warn">{t('set.restart_pending_2')}</span>
        {/if}
        <button class="btn" onclick={reset} disabled={changed.length === 0}>{t('set.revert')}</button>
        <button class="btn primary" onclick={save} disabled={saving || changed.length === 0}>
          {saving ? (t('set.saving')) : (t('set.save_settings'))}
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  /* Tabovi iznad: jedna sekcija u fokusu, ostalo je jedan klik daleko. */
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 0 0 12px;
    border-bottom: 1px solid var(--border, #262b35);
    margin-bottom: 16px;
    position: sticky;
    top: 0;
    z-index: 5;
    background: var(--background, #0b0d12);
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 7px;
    border: 1px solid var(--border, #262b35);
    background: color-mix(in srgb, var(--card, #14171d) 70%, transparent);
    color: inherit;
    font: inherit;
    font-size: 0.9rem;
    border-radius: 999px;
    padding: 0.4rem 0.85rem;
    cursor: pointer;
  }
  .tab:hover {
    border-color: color-mix(in srgb, #22c55e 45%, var(--border, #262b35));
  }
  .tab.active {
    border-color: #22c55e;
    background: color-mix(in srgb, #22c55e 16%, transparent);
    color: #4ade80;
    font-weight: 600;
  }
  .tab .n {
    font-size: 0.7rem;
    opacity: 0.6;
    font-variant-numeric: tabular-nums;
  }
  .tab .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--warn, #fbbf24);
  }
  @media (max-width: 700px) {
    .tabs {
      overflow-x: auto;
      flex-wrap: nowrap;
      padding-bottom: 8px;
    }
    .tab {
      white-space: nowrap;
    }
  }
</style>
