<script>
  // Bento kartica: povlačenje za redoslijed, povlačenje donjeg ruba za širinu,
  // skupljanje klikom. Sve tri stvari se pamte (vidi lib/layout.svelte.js).
  import { layout, move, setSpan, toggleCollapse, grid, effectiveSpan, stepUnits } from '../lib/layout.svelte.js'

  let { id, title, span = 4, collapsed = false, children, actions } = $props()

  /// Širina u stupcima **trenutne** mreže (spremljeno je u 12 stupaca).
  const width = $derived(effectiveSpan(span))

  let resizing = $state(false)
  let over = $state(false)

  function onDragStart(event) {
    layout.dragging = id
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', id)
  }

  function onDragOver(event) {
    if (!layout.dragging || layout.dragging === id) return
    event.preventDefault()
    over = true
  }

  function onDrop(event) {
    event.preventDefault()
    over = false
    move(layout.dragging, id)
    layout.dragging = null
  }

  /// Povlačenje donjeg ruba mijenja širinu u stupcima **trenutne** mreže.
  function startResize(event) {
    event.preventDefault()
    event.stopPropagation()
    const box = event.currentTarget.closest('.bento')
    if (!box) return
    const columns = grid.columns
    const styles = getComputedStyle(box)
    const gap = parseFloat(styles.columnGap) || 0
    const total = box.getBoundingClientRect().width
    const columnWidth = (total - gap * (columns - 1)) / columns
    const startX = event.clientX
    const startSpan = span
    const units = stepUnits()
    resizing = true

    const onMove = (moveEvent) => {
      const delta = moveEvent.clientX - startX
      const steps = Math.round(delta / (columnWidth + gap))
      setSpan(id, startSpan + steps * units)
    }
    const onUp = () => {
      resizing = false
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }
</script>

<div
  class="card"
  role="group"
  aria-label={title}
  class:dragging={layout.dragging === id}
  class:over
  class:collapsed
  style="--span: {width}"
  draggable="true"
  ondragstart={onDragStart}
  ondragover={onDragOver}
  ondragleave={() => (over = false)}
  ondrop={onDrop}
  ondragend={() => (layout.dragging = null)}
>
  <div class="card-head">
    <span class="grip" aria-hidden="true">⣿</span>
    <span class="card-title">{title}</span>
    <span class="card-actions">
      {#if actions}{@render actions()}{/if}
      <button class="btn ghost" title="Skupljeno / rašireno" onclick={() => toggleCollapse(id)}>
        {collapsed ? '▸' : '▾'}
      </button>
    </span>
  </div>
  <div class="card-body">{@render children()}</div>
  <div
    class="resize"
    role="separator"
    aria-orientation="horizontal"
    aria-label="Širina kartice"
    tabindex="0"
    title="Povuci za širinu (ili strelice lijevo/desno)"
    onpointerdown={startResize}
    onkeydown={(event) => {
      if (event.key === 'ArrowLeft') setSpan(id, span - 1)
      if (event.key === 'ArrowRight') setSpan(id, span + 1)
    }}
  >═</div>
</div>
