// Raspored kartica na pregledu: povlačenje, širina i skupljanje — pamti se u browseru.
//
// Zašto localStorage, a ne server: raspored je stvar ekrana na kojem gledaš
// (telefon vs. desktop), a povlačenje mora biti trenutno (bez čekanja mreže).

const KEY = 'rustiio.layout.v1'
const MIN_SPAN = 3
const MAX_SPAN = 12

export const DEFAULT_CARDS = [
  { id: 'streams', span: 5 },
  { id: 'system', span: 4 },
  { id: 'library', span: 3 },
  { id: 'disks', span: 4 },
  { id: 'posters', span: 4 },
  { id: 'logs', span: 4 },
]

function load() {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return DEFAULT_CARDS.map((card) => ({ ...card, collapsed: false }))
    const saved = JSON.parse(raw)
    // Nepoznate kartice se dodaju na kraj, poznate zadržavaju spremljeni redoslijed.
    const known = new Map(saved.map((card) => [card.id, card]))
    const merged = DEFAULT_CARDS.map((card) => ({ ...card, ...known.get(card.id), collapsed: !!known.get(card.id)?.collapsed }))
    const extra = saved.filter((card) => !DEFAULT_CARDS.some((knownCard) => knownCard.id === card.id))
    return [...merged, ...extra]
  } catch {
    return DEFAULT_CARDS.map((card) => ({ ...card, collapsed: false }))
  }
}

export const layout = $state({ cards: load(), dragging: null })

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
