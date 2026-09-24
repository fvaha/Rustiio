// Raspored kartica na pregledu: povlačenje, širina i skupljanje — pamti se u browseru.
//
// Zašto localStorage, a ne server: raspored je stvar ekrana na kojem gledaš
// (telefon vs. desktop), a povlačenje mora biti trenutno (bez čekanja mreže).

const KEY = 'rustiio.layout.v1'
const MIN_SPAN = 3
const MAX_SPAN = 12

export const DEFAULT_CARDS = [
  { id: 'system', span: 8 },
  { id: 'streams', span: 4 },
  { id: 'library', span: 4 },
  { id: 'posters', span: 4 },
  { id: 'disks', span: 4 },
  { id: 'devices', span: 6 },
  { id: 'transcode', span: 6 },
  { id: 'logs', span: 12 },
]

function load() {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return DEFAULT_CARDS.map((card) => ({ ...card, collapsed: false }))
    const saved = JSON.parse(raw)
    // Spremljeni redoslijed se čuva; nove kartice (iz novije verzije) idu na kraj.
    const defaults = new Map(DEFAULT_CARDS.map((card) => [card.id, card]))
    const ordered = []
    for (const card of saved) {
      const base = defaults.get(card.id)
      if (base) defaults.delete(card.id)
      ordered.push({ ...(base ?? { span: 4 }), ...card, collapsed: !!card.collapsed })
    }
    for (const rest of defaults.values()) ordered.push({ ...rest, collapsed: false })
    return ordered.filter((card) => DEFAULT_CARDS.some((known) => known.id === card.id))
  } catch {
    return DEFAULT_CARDS.map((card) => ({ ...card, collapsed: false }))
  }
}

export const layout = $state({ cards: load(), dragging: null })

/// Broj stupaca mreže u ovom trenutku — ista mjesta prijelaza kao u `app.css`.
/// Bez ovoga kartica sa spremljenih 8 stupaca na uskom ekranu (6 stupaca)
/// prelijeva red i raspored izgleda razbacano.
export const grid = $state({ columns: 12 })

const BREAKPOINTS = [
  { query: '(max-width: 640px)', columns: 1 },
  { query: '(max-width: 1000px)', columns: 6 },
]

function measure() {
  if (typeof window === 'undefined') return
  const hit = BREAKPOINTS.find((bp) => window.matchMedia(bp.query).matches)
  grid.columns = hit ? hit.columns : 12
}

measure()
if (typeof window !== 'undefined') {
  for (const bp of BREAKPOINTS) {
    window.matchMedia(bp.query).addEventListener('change', measure)
  }
}

/// Širina u stupcima **trenutne** mreže (spremljeno je uvijek u 12 stupaca).
export function effectiveSpan(rawSpan) {
  const raw = rawSpan ?? 4
  if (grid.columns >= 12) return raw
  return Math.max(1, Math.min(grid.columns, Math.round((raw * grid.columns) / 12)))
}

/// Jedan vizualni korak povlačenja, u 12-stupčanim jedinicama.
export function stepUnits() {
  return Math.max(1, Math.round(12 / grid.columns))
}

function persist() {
  try {
    localStorage.setItem(KEY, JSON.stringify(layout.cards.map(({ id, span, collapsed }) => ({ id, span, collapsed }))))
  } catch {
    /* privatni način rada — raspored se ne pamti, ne prekidamo rad */
  }
}

export function cardOf(id) {
  return layout.cards.find((card) => card.id === id)
}

export function move(fromId, toId) {
  if (!fromId || fromId === toId) return
  const from = layout.cards.findIndex((card) => card.id === fromId)
  const to = layout.cards.findIndex((card) => card.id === toId)
  if (from < 0 || to < 0) return
  const cards = [...layout.cards]
  const [moved] = cards.splice(from, 1)
  cards.splice(to, 0, moved)
  layout.cards = cards
  persist()
}

export function setSpan(id, span) {
  const card = cardOf(id)
  if (!card) return
  card.span = Math.min(MAX_SPAN, Math.max(MIN_SPAN, Math.round(span)))
  layout.cards = [...layout.cards]
  persist()
}

export function toggleCollapse(id) {
  const card = cardOf(id)
  if (!card) return
  card.collapsed = !card.collapsed
  layout.cards = [...layout.cards]
  persist()
}

export function resetLayout() {
  layout.cards = DEFAULT_CARDS.map((card) => ({ ...card, collapsed: false }))
  persist()
}
