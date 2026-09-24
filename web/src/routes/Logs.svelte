<script>
  // Zapisnik: živi tok preko WebSocketa, filtri, pauza, preuzimanje.
  import { t } from '../lib/i18n.svelte.js'
  import { store, toast } from '../lib/store.svelte.js'
  import { clockTime } from '../lib/format.js'

  let level = $state('')
  let needle = $state('')
  let follow = $state(true)
  let container

  const filtered = $derived(
    store.logs.filter((line) => {
      if (level && line.level !== level) return false
      if (needle && !`${line.message} ${line.target}`.toLowerCase().includes(needle.toLowerCase())) return false
      return true
    }),
  )

  // Prati dno dok korisnik sam ne odmakne (ili ne pauzira).
  $effect(() => {
    if (!follow || !container) return
    filtered.length
    container.scrollTop = container.scrollHeight
  })

  function onScroll() {
    if (!container) return
    const atBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 40
    follow = atBottom
  }

  function download() {
    const text = filtered.map((line) => `${clockTime(line.at_ms)} ${line.level} ${line.target} ${line.message}`).join('\n')
    const url = URL.createObjectURL(new Blob([text], { type: 'text/plain' }))
    const link = document.createElement('a')
    link.href = url
    link.download = `rustiio-zapisnik-${Date.now()}.log`
    link.click()
    URL.revokeObjectURL(url)
  }

  async function copyLine(line) {
    try {
      await navigator.clipboard.writeText(`${clockTime(line.at_ms)} ${line.level} ${line.target} ${line.message}`)
      toast('ok', t('common.copied'))
    } catch {
      toast('warn', line.message)
    }
  }
</script>

<div class="lib-bar">
  <span class="pill">
    <span class="dot" class:off={store.logSocket !== 'open'}></span>
    {store.logSocket === 'open' ? t('logs.connected') : t('logs.disconnected')}
  </span>
  <input class="field" style="flex: 1 1 200px" placeholder={t('logs.filter')} bind:value={needle} />
  <select class="field" bind:value={level}>
    <option value="">{t('logs.level')}: {t('common.all')}</option>
    <option value="INFO">INFO</option>
    <option value="WARN">WARN</option>
    <option value="ERROR">ERROR</option>
    <option value="DEBUG">DEBUG</option>
  </select>
  <button class="btn" class:primary={!store.logsPaused} onclick={() => (store.logsPaused = !store.logsPaused)}>
    {store.logsPaused ? t('logs.paused') : t('logs.live')}
  </button>
  <span class="spacer"></span>
  <span class="pill">{filtered.length} / {store.logs.length}</span>
  <button class="btn ghost" onclick={() => (store.logs = [])}>{t('logs.clear')}</button>
  <button class="btn" onclick={download}>{t('logs.download')}</button>
</div>

{#if store.logsPaused}
  <div class="muted small" style="margin-bottom: 8px">{t('logs.paused_hint')}</div>
{/if}

<div class="card">
  <div class="card-body">
    <div class="logs" bind:this={container} onscroll={onScroll}>
      {#each filtered as line, index (index)}
        <div
          class="log"
          class:error={line.level === 'ERROR'}
          class:warn={line.level === 'WARN'}
          onclick={() => copyLine(line)}
          title={t('common.copy')}
        >
          <span class="t">{clockTime(line.at_ms)}</span>
          <span class="lvl {line.level}">{line.level}</span>
          <span class="msg">{line.message}</span>
        </div>
      {:else}
        <div class="empty">{t('common.none')}</div>
      {/each}
    </div>
  </div>
</div>
