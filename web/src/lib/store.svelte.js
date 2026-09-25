// Središnje stanje sučelja: podaci sa servera, zapisnik i poruke korisniku.
//
// Pravilo: nijedna greška se ne gubi — ide u toast i u `store.error` (korisnik to traži).

import { get, post, openLogs, put } from './api.js'
import { setLocale, t } from './i18n.svelte.js'
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
  /// Server trenutno ne odgovara (restart ili mreža) — tada se greške ne prikazuju.
  offline: false,
  /// Korisnik je kliknuo restart i čekamo da se servis vrati.
  restarting: false,
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

/// Je li greška samo „server nije tu“ (restart, prekinuta veza) — to nije kvar
/// i ne smije završiti kao crveni toast.
function nedostupan(error) {
  if (store.restarting) return true
  const tekst = String(error?.message ?? error ?? '')
  return error instanceof TypeError || /failed to (load|fetch)|networkerror|load failed|err_connection/i.test(tekst)
}

/// Server je stao: jedna tiha obavijest, pa čekamo povratak (bez ponavljanja).
function stao() {
  store.error = null
  if (!store.offline) {
    store.offline = true
    toast('warn', t('err.server_down'), 12000)
  }
}

/// Server je opet tu — javi jednom i očisti zastavicu.
function vratioSe() {
  if (store.offline) {
    store.offline = false
    toast('ok', t('err.server_back'))
  }
}

function fail(kljuc, error) {
  if (nedostupan(error)) {
    stao()
    return
  }
  store.error = `${t(kljuc)}: ${error?.message || error}`
  toast('err', store.error)
}

export async function refreshStatus() {
  try {
    store.status = await get('/api/status')
    vratioSe()
  } catch (error) {
    fail('err.status', error)
  }
}

export async function refreshStreams() {
  try {
    store.streams = await get('/api/streams')
    vratioSe()
  } catch (error) {
    fail('err.streams', error)
  }
}

export async function refreshStats() {
  try {
    const stats = await get('/api/stats')
    vratioSe()
    store.stats = stats
    if (typeof stats?.cpu?.usage === 'number') {
      store.history.cpu = [...store.history.cpu, stats.cpu.usage].slice(-60)
    }
    if (stats?.memory?.total) {
      const used = (stats.memory.used / stats.memory.total) * 100
      store.history.memory = [...store.history.memory, used].slice(-60)
    }
  } catch (error) {
    fail('err.stats', error)
  }
}

export async function refreshDevices() {
  try {
    store.devices = await get('/api/devices')
    vratioSe()
  } catch (error) {
    fail('err.devices', error)
  }
}

export async function refreshLibrary() {
  try {
    store.library = await get('/api/library')
    vratioSe()
  } catch (error) {
    fail('err.library', error)
  }
}

export async function refreshProfiles() {
  try {
    store.profiles = await get('/api/profiles')
    vratioSe()
  } catch (error) {
    fail('err.profiles', error)
  }
}

export async function rescan() {
  try {
    const result = await post('/api/rescan')
    toast('ok', `${t('common.scan')}: ${result?.counts ? JSON.stringify(result.counts) : t('common.started')}`)
    await Promise.all([refreshStatus(), refreshLibrary()])
  } catch (error) {
    fail('err.rescan', error)
  }
}

export async function refreshPosters() {
  try {
    const result = await post('/api/posters/refresh')
    toast('ok', `${t('common.posters')}: ${result?.obradeno ?? 0} / ${result?.dohvaceno ?? 0}`)
    await refreshStatus()
  } catch (error) {
    fail('err.posters', error)
  }
}

export async function saveSettings(config) {
  const result = await put('/api/settings', config)
  toast('ok', t('common.saved'))
  if (result?.restart_potreban) {
    toast('warn', `Restart potreban za: ${result.promijenjena.slice(1).join(', ') || 'port/adresa'}`, 9000)
  }
  await refreshStatus()
  return result
}

/// Restart servisa: pričekaj da stvarno odgovori, pa ponovno dohvati sve.
/// Prije se stranica sama osvježavala nakon 6 s — ako je servis bio sporiji,
/// sučelje bi dočekalo prazno stanje i niz crvenih toastova.
export async function restartServer() {
  store.restarting = true
  try {
    await post('/api/restart')
  } catch (error) {
    if (!nedostupan(error)) {
      store.restarting = false
      fail('err.restart', error)
      return
    }
  }
  toast('warn', t('settings.restarting'), 60000)

  const pocetak = Date.now()
  const provjeri = async () => {
    if (Date.now() - pocetak > 90000) {
      store.restarting = false
      store.error = t('err.restart_timeout')
      toast('err', store.error, 15000)
      return
    }
    try {
      await get('/api/status')
    } catch {
      setTimeout(provjeri, 1000)
      return
    }
    store.restarting = false
    store.offline = false
    stop()
    await start({ force: true })
    toast('ok', t('err.server_back'))
  }
  setTimeout(provjeri, 1500)
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

export async function start({ force = false } = {}) {
  if (started && !force) return
  started = true
  stop()

  await refreshStatus()
  setLocale(store.status?.ui_language ?? 'auto')
  await Promise.all([refreshStats(), refreshStreams(), refreshDevices(), refreshLibrary()])

  // Zadržane linije prije živog toka (da UI nije prazan prvih sekundi).
  try {
    const logs = await get('/api/logs?limit=200')
    store.logs = (logs?.lines ?? []).map((line) => ({ ...line, at: clockTime(line.at_ms) }))
  } catch (error) {
    fail('err.logs', error)
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
