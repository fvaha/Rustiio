<script>
  // Crta kretanja: punjena površina s gradijentom + točka na zadnjoj vrijednosti.
  // SVG, bez biblioteke; `id` treba biti jedinstven po instanci (gradijenti u SVG-u su globalni).
  let { values = [], max = 100, height = 40, color = 'var(--accent)', id = 'spark' } = $props()

  const data = $derived(values.length ? values : [0])

  const line = $derived.by(() => {
    const width = 100
    const step = data.length > 1 ? width / (data.length - 1) : width
    return data
      .map((value, index) => {
        const ratio = Math.min(1, Math.max(0, (Number(value) || 0) / max))
        return `${(index * step).toFixed(2)},${(height - ratio * height).toFixed(2)}`
      })
      .join(' ')
  })

  const area = $derived(`0,${height} ${line} 100,${height}`)

  const last = $derived.by(() => {
    const lastValue = Number(data[data.length - 1]) || 0
    const ratio = Math.min(1, Math.max(0, lastValue / max))
    const x = data.length > 1 ? 100 : 0
    return { x: x.toFixed(2), y: (height - ratio * height).toFixed(2) }
  })
</script>

<svg viewBox="0 0 100 {height}" preserveAspectRatio="none" style="width: 100%; height: {height}px; display: block" aria-hidden="true">
  <defs>
    <linearGradient id="fill-{id}" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color={color} stop-opacity="0.32" />
      <stop offset="100%" stop-color={color} stop-opacity="0" />
    </linearGradient>
  </defs>
  <polygon points={area} fill="url(#fill-{id})" />
  <polyline points={line} fill="none" stroke={color} stroke-width="1.4" vector-effect="non-scaling-stroke" stroke-linejoin="round" />
  <circle cx={last.x} cy={last.y} r="2" fill={color} vector-effect="non-scaling-stroke" />
</svg>
