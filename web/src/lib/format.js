// Sitni formati — brojevi se u UI-u čitaju, ne prepričavaju.

export function bytes(value) {
  const number = Number(value) || 0
  if (number < 1024) return `${number} B`
  const units = ['kB', 'MB', 'GB', 'TB', 'PB']
  let size = number / 1024
  let unit = 0
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024
    unit += 1
  }
  return `${size >= 100 ? size.toFixed(0) : size.toFixed(1)} ${units[unit]}`
}

export function uptime(seconds) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0))
  const days = Math.floor(total / 86400)
  const hours = Math.floor((total % 86400) / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  if (days > 0) return `${days}d ${hours}h`
  if (hours > 0) return `${hours}h ${minutes}min`
  if (minutes > 0) return `${minutes}min ${total % 60}s`
  return `${total}s`
}

export function duration(seconds) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const rest = total % 60
  const pad = (value) => String(value).padStart(2, '0')
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(rest)}` : `${minutes}:${pad(rest)}`
}

export function percent(value, digits = 0) {
  return `${(Number(value) || 0).toFixed(digits)}%`
}

export function bitrate(bitsPerSecond) {
  const value = Number(bitsPerSecond) || 0
  if (value <= 0) return '—'
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)} Mb/s`
  return `${Math.round(value / 1000)} kb/s`
}

const pad = (value) => String(value).padStart(2, '0')

export function clockTime(epochMs) {
  const date = new Date(Number(epochMs) || 0)
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

export function dateTime(epochMs) {
  if (!epochMs) return '—'
  const date = new Date(Number(epochMs))
  return `${pad(date.getDate())}.${pad(date.getMonth() + 1)}.${date.getFullYear()} ${clockTime(epochMs)}`
}

/// "prije 3 min" — za uređaje i zapise.
export function ago(epochMs) {
  const then = Number(epochMs) || 0
  if (!then) return '—'
  const seconds = Math.max(0, Math.round((Date.now() - then) / 1000))
  if (seconds < 60) return `${seconds}s`
  if (seconds < 3600) return `${Math.round(seconds / 60)}min`
  if (seconds < 86400) return `${Math.round(seconds / 3600)}h`
  return `${Math.round(seconds / 86400)}d`
}

export function barClass(value) {
  const level = Number(value) || 0
  if (level >= 92) return 'err'
  if (level >= 78) return 'warn'
  return ''
}
