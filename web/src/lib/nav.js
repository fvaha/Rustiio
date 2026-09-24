// Popis stranica sučelja — dijeli ga navigacija i naslov u zaglavlju.
export const TABS = [
  { id: 'dashboard', icon: '◈' },
  { id: 'library', icon: '▤' },
  { id: 'devices', icon: '⛁' },
  { id: 'logs', icon: '≡' },
  { id: 'settings', icon: '⚙' },
]

export const TAB_IDS = TABS.map((tab) => tab.id)
