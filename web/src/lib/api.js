// Tanki sloj nad REST-om: svaka greška nosi poruku koju UI pokaže (nikad tiho).

async function parse(response) {
  const text = await response.text()
  let data = null
  if (text) {
    try {
      data = JSON.parse(text)
    } catch {
      data = { poruka: text }
    }
  }
  if (!response.ok) {
    const message = data?.greska || data?.error || `HTTP ${response.status}`
    throw new Error(message)
  }
  return data
}

export async function get(path, options = {}) {
  return parse(await fetch(path, { headers: { accept: 'application/json' }, ...options }))
}

export async function post(path, body) {
  return parse(
    await fetch(path, {
      method: 'POST',
      headers: body ? { 'content-type': 'application/json' } : {},
      body: body ? JSON.stringify(body) : undefined,
    }),
  )
}

export async function put(path, body) {
  return parse(
    await fetch(path, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    }),
  )
}

/// Živi zapisnik: WebSocket s automatskim ponovnim spajanjem.
/// Vraća funkciju za zatvaranje.
export function openLogs(onLine, onState) {
  let socket = null
  let closed = false
  let retry = 0

  const connect = () => {
    if (closed) return
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
    socket = new WebSocket(`${scheme}://${location.host}/ws/logs`)
    socket.onopen = () => {
      retry = 0
      onState?.('open')
    }
    socket.onmessage = (event) => {
      try {
        onLine(JSON.parse(event.data))
      } catch {
        /* neispravna linija se preskače */
      }
    }
    socket.onerror = () => onState?.('error')
    socket.onclose = () => {
      onState?.('closed')
      if (closed) return
      retry = Math.min(retry + 1, 6)
      setTimeout(connect, 500 * retry)
    }
  }

  connect()
  return () => {
    closed = true
    socket?.close()
  }
}
