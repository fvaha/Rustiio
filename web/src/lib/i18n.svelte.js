// Prijevod (hr/en) — od početka, jer se poslije ne vraća.
//
// Zadano `auto`: jezik se uzima iz preglednika; izbor se pamti u `config.ui.language`.

/// Trenutni jezik; `auto` se razrješava iz preglednika.

import { dictionaries } from './strings.js'

export const i18n = $state({ lang: 'hr', choice: 'auto' })

export function setLocale(choice) {
  i18n.choice = choice || 'auto'
  if (i18n.choice === 'auto') {
    const browser = (navigator.language || 'hr').slice(0, 2).toLowerCase()
    i18n.lang = browser === 'en' ? 'en' : 'hr'
  } else {
    i18n.lang = dictionaries[i18n.choice] ? i18n.choice : 'hr'
  }
  document.documentElement.lang = i18n.lang
}

export function t(path) {
  const parts = path.split('.')
  let value = dictionaries[i18n.lang] ?? dictionaries.hr
  for (const part of parts) {
    value = value?.[part]
    if (value === undefined) return path
  }
  return value
}

export const languages = [
  { id: 'auto', label: () => (i18n.lang === 'en' ? 'Auto' : 'Automatski') },
  { id: 'hr', label: () => 'Hrvatski' },
  { id: 'en', label: () => 'English' },
]
