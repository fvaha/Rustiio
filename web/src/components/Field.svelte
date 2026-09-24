<script>
  // Jedno polje configa: nacrtaj kontrolu prema opisu iz settings-schema.js.
  // Vrijednost se mijenja samo kroz onchange (roditelj drži config).
  import { i18n } from '../lib/i18n.svelte.js'
  import { ROOT_KINDS } from '../lib/settings-schema.js'

  let { path, meta, value, changed = false, onchange } = $props()

  const lang = $derived(i18n.lang === 'en' ? 'en' : 'hr')
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
</script>

<div class="field" class:changed data-field={path}>
  <div class="label">
    <span class="t">
      {label}
      {#if changed}<span class="badge info" title="Promijenjeno, još nije spremljeno">•</span>{/if}
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
        <span class="hint">{value === true ? (lang === 'en' ? 'on' : 'uključeno') : (lang === 'en' ? 'off' : 'isključeno')}</span>
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
            <button type="button" title={lang === 'en' ? 'Remove' : 'Ukloni'} onclick={() => removeTag(tag)}>×</button>
          </span>
        {/each}
        <input
          class="input mono"
          style="width: 130px"
          placeholder={lang === 'en' ? 'add…' : 'dodaj…'}
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
            <input class="input" placeholder={lang === 'en' ? 'label' : 'naziv'} value={root.label ?? ''} onchange={(event) => setRoot(index, 'label', event.currentTarget.value)} />
            <input class="input mono" placeholder="/putanja/do/mape" value={root.path ?? ''} onchange={(event) => setRoot(index, 'path', event.currentTarget.value)} />
            <select class="select" value={root.kind ?? 'video'} onchange={(event) => setRoot(index, 'kind', event.currentTarget.value)}>
              {#each ROOT_KINDS as kind}
                <option value={kind.id}>{text(kind.label)}</option>
              {/each}
            </select>
            <button class="btn ghost danger" type="button" title={lang === 'en' ? 'Remove' : 'Ukloni mapu'} onclick={() => removeRoot(index)}>✕</button>
          </div>
        {/each}
        <div><button class="btn sm" type="button" onclick={addRoot}>+ {lang === 'en' ? 'Add folder' : 'Dodaj mapu'}</button></div>
      </div>

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
