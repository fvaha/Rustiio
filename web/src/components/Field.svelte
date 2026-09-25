<script>
  // Jedno polje configa: nacrtaj kontrolu prema opisu iz settings-schema.js.
  // Vrijednost se mijenja samo kroz onchange (roditelj drži config).
  import { t, i18n } from '../lib/i18n.svelte.js'
  import FolderPicker from './FolderPicker.svelte'
  import Icon from './Icon.svelte'

  let { path, meta, value, changed = false, onchange, hw = null } = $props()

  // Jezik polja dolazi iz odabranog jezika sučelja. Prije je pisalo
  // `t('field.en')`, a to nije ključ u rječniku pa `t()` vrati sam tekst
  // „field.en" i `text()` uvijek padne na hrvatski — polja su ostajala
  // hrvatska i kad je sučelje na engleskom.
  const lang = $derived(i18n.lang)
  const text = (item) => (item && typeof item === 'object' ? (item[lang] ?? item.hr ?? '') : (item ?? ''))
  const type = $derived(meta.type)
  const label = $derived(text(meta.label))
  const help = $derived(text(meta.help))

  let draft = $state('')
  let editing = $state(false)
  let tagInput = $state('')
  let jsonText = $state('')
  let jsonError = $state('')

  $effect(() => {
    // Kad se vrijednost promijeni izvana (ponovno učitavanje), uskladi lokalni tekst.
    if (!editing) draft = value === null || value === undefined ? '' : String(value)
  })

  $effect(() => {
    if (type === 'json') jsonText = JSON.stringify(value ?? [], null, 2)
  })

  function commitText() {
    editing = false
    if (type === 'number') {
      const number = Number(draft)
      onchange(Number.isFinite(number) ? number : 0)
    } else {
      onchange(draft)
    }
  }

  function addTag() {
    const tag = tagInput.trim().replace(/^\./, '')
    if (!tag) return
    const list = Array.isArray(value) ? value : []
    if (!list.includes(tag)) onchange([...list, tag])
    tagInput = ''
  }

  function removeTag(tag) {
    onchange((Array.isArray(value) ? value : []).filter((item) => item !== tag))
  }

  function commitJson() {
    try {
      const parsed = JSON.parse(jsonText)
      jsonError = ''
      onchange(parsed)
    } catch (error) {
      jsonError = String(error.message)
    }
  }

  function setRoot(index, key, next) {
    const list = (Array.isArray(value) ? value : []).map((item) => ({ ...item }))
    list[index][key] = next
    onchange(list)
  }

  function removeRoot(index) {
    onchange((Array.isArray(value) ? value : []).filter((_, item) => item !== index))
  }

  function addRoot() {
    onchange([...(Array.isArray(value) ? value : []), { label: '', path: '', kind: 'video' }])
  }

  /// Biranje mape kroz sustav: -1 zatvoreno, -2 nova mapa, inače indeks retka.
  let pickFor = $state(-1)

  /// Ime mape iz putanje ("/home/user/Filmovi" → "Filmovi").
  function folderName(path) {
    const parts = String(path).split(/[/\\]/).filter(Boolean)
    return parts.length ? parts[parts.length - 1] : ''
  }

  function startPicking(index) {
    pickFor = index
  }

  function picked(path) {
    if (pickFor === -2) {
      onchange([
        ...(Array.isArray(value) ? value : []),
        { label: folderName(path), path, kind: 'video' },
      ])
    } else if (pickFor >= 0) {
      setRoot(pickFor, 'path', path)
      const current = (Array.isArray(value) ? value : [])[pickFor]
      if (!current?.label?.trim()) setRoot(pickFor, 'label', folderName(path))
    }
    pickFor = -1
  }

  /// Putanja od koje preglednik kreće: postojeći unos ili početna mapa.
  const pickStart = $derived(
    pickFor >= 0 && Array.isArray(value) ? (value[pickFor]?.path ?? '') : '',
  )

  /// Server traži apsolutnu putanju — pokaži to odmah, ne tek pri spremanju.
  const isAbsolute = (candidate) =>
    String(candidate ?? '').startsWith('/') || /^[A-Za-z]:[/\\]/.test(String(candidate ?? ''))
</script>

<div class="field" class:changed data-field={path}>
  <div class="label">
    <span class="t">
      {label}
      {#if changed}<span class="badge info" title={t('field.changed_unsaved')}>•</span>{/if}
    </span>
    <span class="h">{help}</span>
  </div>

  <div class="control">
    {#if type === 'bool'}
      <div style="display: flex; align-items: center; gap: 10px">
        <button
          class="switch"
          class:on={value === true}
          role="switch"
          aria-checked={value === true}
          aria-label={label}
          onclick={() => onchange(!value)}
        ></button>
        <span class="hint">{value === true ? t('field.on') : t('field.off')}</span>
      </div>

    {:else if type === 'cards'}
      <div class="izbor">
        {#each meta.options ?? [] as option}
          {@const vrijednost = typeof option === 'object' ? option.value : option}
          {@const dostupno = !hw?.modes || hw.modes.includes(vrijednost)}
          <button
            class="izbor-kartica"
            class:aktivno={String(value ?? '') === String(vrijednost)}
            disabled={!dostupno}
            onclick={() => onchange(vrijednost)}
          >
            <span class="izbor-naslov">{text(option.label) || vrijednost}</span>
            <span class="izbor-opis">{text(option.hint)}</span>
            {#if !dostupno}<span class="izbor-znak">{t('field.unavailable')}</span>{/if}
          </button>
        {/each}
      </div>

    {:else if type === 'cores'}
      {@const najvise = hw?.cpu?.threads ?? meta.max ?? 32}
      {@const prazno = value === null || value === undefined || value === 0}
      {@const odabrano = prazno ? najvise : Math.min(value, najvise)}
      <div class="jezgre">
        <div class="jezgre-vrh">
          <span class="jezgre-broj">{odabrano}</span>
          <span class="hint">{prazno ? t('field.all_cores') : t('field.cores_word')}</span>
          <span class="muted small">/ {najvise}</span>
        </div>
        <div class="jezgre-traka">
          {#each Array(najvise) as _, index}
            <button
              class="jezgra"
              class:upaljena={index < odabrano}
              title={String(index + 1)}
              aria-label={String(index + 1)}
              onclick={() => onchange(index + 1)}
            ></button>
          {/each}
        </div>
        <div class="jezgre-brzi">
          <button class="btn ghost" onclick={() => onchange(najvise)}>{t('field.all')}</button>
          <button class="btn ghost" onclick={() => onchange(Math.max(1, Math.floor(najvise / 2)))}>½</button>
          <button class="btn ghost" onclick={() => onchange(0)}>{t('field.auto')}</button>
        </div>
      </div>

    {:else if type === 'enum'}
      <select class="select" style="max-width: 260px" value={value ?? ''} onchange={(event) => onchange(event.currentTarget.value)}>
        {#each meta.options ?? [] as option}
          <option value={option}>{option}</option>
        {/each}
        {#if meta.options && !meta.options.includes(value)}
          <option value={value}>{value}</option>
        {/if}
      </select>

    {:else if type === 'number'}
      <div style="display: flex; align-items: center; gap: 8px">
        <input
          class="input num"
          type="number"
          style="max-width: 150px"
          min={meta.min}
          max={meta.max}
          step={meta.step ?? 1}
          bind:value={draft}
          onfocus={() => (editing = true)}
          onblur={commitText}
          onkeydown={(event) => event.key === 'Enter' && commitText()}
        />
        {#if meta.unit}<span class="hint">{text(meta.unit)}</span>{/if}
        {#if meta.min !== undefined}<span class="hint">{meta.min}–{meta.max ?? '∞'}</span>{/if}
      </div>

    {:else if type === 'tags'}
      <div class="tags">
        {#each Array.isArray(value) ? value : [] as tag}
          <span class="tag-item">
            {tag}
            <button type="button" title={t('common.remove')} onclick={() => removeTag(tag)}>×</button>
          </span>
        {/each}
        <input
          class="input mono"
          style="width: 130px"
          placeholder={t('field.tag_add')}
          bind:value={tagInput}
          onkeydown={(event) => {
            if (event.key === 'Enter' || event.key === ',') {
              event.preventDefault()
              addTag()
            }
          }}
          onblur={addTag}
        />
      </div>

    {:else if type === 'roots'}
      <div class="roots">
        {#each Array.isArray(value) ? value : [] as root, index}
          <div class="root-row">
            <input class="input" placeholder={t('field.root_label')} value={root.label ?? ''} onchange={(event) => setRoot(index, 'label', event.currentTarget.value)} />
            <input
              class="input mono"
              class:bad={root.path && !isAbsolute(root.path)}
              title={root.path && !isAbsolute(root.path) ? t('field.absolute_hint') : ''}
              placeholder="/putanja/do/mape"
              value={root.path ?? ''}
              onchange={(event) => setRoot(index, 'path', event.currentTarget.value)}
            />
            <button
              class="btn"
              type="button"
              title={t('field.browse')}
              onclick={() => startPicking(index)}
            ><Icon name="folder" size={14} /> {t('field.choose')}</button>
            <button class="btn ghost danger" type="button" title={t('field.remove_root')} onclick={() => removeRoot(index)}>✕</button>
          </div>
        {/each}
        <div class="roots-actions">
          <button class="btn primary sm" type="button" onclick={() => startPicking(-2)}>
            + {t('field.add_folder')}
          </button>
          <button class="btn ghost sm" type="button" title={t('field.add_row')} onclick={addRoot}>
            {t('field.type_path')}
          </button>
        </div>
      </div>

      <FolderPicker
        open={pickFor !== -1}
        start={pickStart}
        {lang}
        onpick={picked}
        onclose={() => (pickFor = -1)}
      />

    {:else if type === 'json'}
      <textarea class="textarea mono" rows="6" bind:value={jsonText} onblur={commitJson}></textarea>
      {#if jsonError}<span class="hint" style="color: var(--err)">{jsonError}</span>{/if}

    {:else}
      <input
        class="input"
        class:mono={meta.mono}
        style="max-width: 460px"
        placeholder={text(meta.placeholder)}
        bind:value={draft}
        onfocus={() => (editing = true)}
        onblur={commitText}
        onkeydown={(event) => event.key === 'Enter' && commitText()}
      />
    {/if}
  </div>
</div>
