<script>
  // Mala crta kretanja (CPU/RAM) — SVG, bez biblioteke.
  let { values = [], max = 100, height = 34, color = 'var(--accent)' } = $props()

  const points = $derived.by(() => {
    const data = values.length ? values : [0]
    const width = 100
    const step = data.length > 1 ? width / (data.length - 1) : width
    return data
      .map((value, index) => {
        const ratio = Math.min(1, Math.max(0, (Number(value) || 0) / max))
        return `${(index * step).toFixed(2)},${(height - ratio * height).toFixed(2)}`
      })
      .join(' ')
  })
</script>

<svg viewBox="0 0 100 {height}" preserveAspectRatio="none" style="width: 100%; height: {height}px; display: block">
  <polyline points={points} fill="none" stroke={color} stroke-width="1.4" vector-effect="non-scaling-stroke" />
</svg>
