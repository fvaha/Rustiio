// Središnje stanje sučelja: podaci sa servera, zapisnik i poruke korisniku.
//
// Pravilo: nijedna greška se ne gubi — ide u toast i u `store.error` (korisnik to traži).

import { get, post, openLogs, put } from './api.js'
import { setLocale } from './i18n.svelte.js'
import { clockTime } from './format.js'

export const store = $state({
  status: null,
  stats: null,
  streams: null,
  devices: null,
  library: null,
  profiles: null,
  history: { cpu: [], memory: [] },
  logs: [],
  logSocket: 'closed',
  logsPaused: false,
  toasts: [],
  error: null,
  csrf: null,
})

const LOG_LIMIT = 800

export function toast(kind, text, ms = 6000) {
  const id = Math.random().toString(36).slice(2)
  store.toasts = [...store.toasts, { id, kind, text }]
  setTimeout(() => {
    store.toasts = store.toasts.filter((item) => item.id !== id)
  }, ms)
}

export function dismissToast(id) {
  store.toasts = store.toasts.filter((item) => item.id !== id)
}

function fail(where, error) {
  store.error = `${where}: ${error?.message || error}`
  toast('err', store.error)
}

export async function refreshStatus() {
  try {
    store.status = await get('/api/status')
  } catch (error) {
    fail('status', error)
  }
}

export async function refreshStreams() {
  try {
    store.streams = await get('/api/streams')
  } catch (error) {
    fail('streams', error)
  }
}

export async function refreshStats() {
  try {
    const stats = await get('/api/stats')
    store.stats = stats
    if (typeof stats?.cpu?.usage === 'number') {
      store.history.cpu = [...store.history.cpu, stats.cpu.usage].slice(-60)
    }
    if (stats?.memory?.total) {
      const used = (stats.memory.used / stats.memory.total) * 100
      store.history.memory = [...store.history.memory, used].slice(-60)
    }
  } catch (error) {
    fail('stats', error)
  }
}

export async function refreshDevices() {
  try {
    store.devices = await get('/api/devices')
  } catch (error) {
    fail('devices', error)
  }
}

export async function refreshLibrary() {
  try {
    store.library = await get('/api/library')
  } catch (error) {
    fail('library', error)
  }
}

export async function refreshProfiles() {
  try {
    store.profiles = await get('/api/profiles')
  } catch (error) {
    fail('profiles', error)
  }
}

export async function rescan() {
  try {
    const result = await post('/api/rescan')
    toast('ok', `Skeniranje: ${result?.counts ? JSON.stringify(result.counts) : 'pokrenuto'}`)
    await Promise.all([refreshStatus(), refreshLibrary()])
  } catch (error) {
    fail('rescan', error)
  }
}

export async function refreshPosters() {
  try {
    const result = await post('/api/posters/refresh')
    toast('ok', `Posteri: ${result?.obradeno ?? 0} obrađeno, ${result?.dohvaceno ?? 0} dohvaćeno`)
    await refreshStatus()
  } catch (error) {
    fail('posteri', error)
  }
}

export async function saveSettings(config) {
  const result = await put('/api/settings', config)
  toast('ok', 'Postavke spremljene')
  if (result?.restart_potreban) {
    toast('warn', `Restart potreban za: ${result.promijenjena.slice(1).join(', ') || 'port/adresa'}`, 9000)
  }
  await refreshStatus()
  return result
}

export async function restartServer() {
  try {
    await post('/api/restart')
    toast('warn', 'Servis se restartа…', 8000)
    setTimeout(() => {
      location.reload()
    }, 6000)
  } catch (error) {
    fail('restart', error)
  }
}

function pushLog(line) {
  if (store.logs.length >= LOG_LIMIT) {
    store.logs = [...store.logs.slice(-Math.floor(LOG_LIMIT * 0.75)), line]
  } else {
    store.logs = [...store.logs, line]
  }
}

let stopLogs = null
let timers = []
let started = false

export async function start() {
  if (started) return
  started = true

  await refreshStatus()
  setLocale(store.status?.ui_language ?? 'auto')
  await Promise.all([refreshStats(), refreshStreams(), refreshDevices(), refreshLibrary()])

  // Zadržane linije prije živog toka (da UI nije prazan prvih sekundi).
  try {
    const logs = await get('/api/logs?limit=200')
    store.logs = (logs?.lines ?? []).map((line) => ({ ...line, at: clockTime(line.at_ms) }))
  } catch (error) {
    fail('zapisnik', error)
  }

  stopLogs = openLogs(
    (line) => pushLog({ ...line, at: clockTime(line.at_ms) }),
    (socketState) => {
      store.logSocket = socketState
    },
  )

  const every = (ms, action) => {
    const id = setInterval(action, ms)
    timers.push(id)
    return id
  }
  every(3000, () => {
    refreshStatus()
    refreshStreams()
  })
  every(4000, refreshStats)
  every(10000, refreshDevices)
  every(30000, refreshLibrary)
}

export function stop() {
  timers.forEach(clearInterval)
  timers = []
  stopLogs?.()
  started = false
}
