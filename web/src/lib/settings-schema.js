// Opis **svih** polja configa: naziv, pomoć, tip kontrole i što traži restart.
//
// Pravilo: polje koje ovdje nema opis se ipak prikaže (tip se izvede iz vrijednosti),
// tako da nijedno podešavanje ne ostane skriveno kad se config proširi.

export const SECTIONS = [
  {
    key: 'server',
    title: { hr: 'Server', en: 'Server' },
    desc: {
      hr: 'Ime pod kojim se server vidi na televizorima, adresa i port, te razina zapisnika.',
      en: 'How the server appears on TVs, its address and port, and log verbosity.',
    },
    icon: '📺',
  },
  {
    key: 'library',
    title: { hr: 'Knjižnica', en: 'Library' },
    desc: {
      hr: 'Mape s medijima, koje se datoteke priznaju, koliko duboko se skenira i što se nudi na televizorima.',
      en: 'Media folders, which files count, how deep to scan, and what TVs are offered.',
    },
    icon: '🗂',
  },
  {
    key: 'transcode',
    title: { hr: 'Transcode', en: 'Transcoding' },
    desc: {
      hr: 'Prekodiranje za televizore koji ne podnose izvorni format. Isključeno znači „nikad ne diraj datoteku".',
      en: 'Re-encoding for TVs that cannot play the source format. Off means "never touch the file".',
    },
    icon: '🎛',
  },
  {
    key: 'profiles',
    title: { hr: 'Profili uređaja', en: 'Device profiles' },
    desc: {
      hr: 'Što server smije ponuditi kojem televizoru i gdje se spremaju profili naučeni iz stvarnih uređaja.',
      en: 'What the server may offer each TV, and where profiles learned from real devices are stored.',
    },
    icon: '🧩',
  },
  {
    key: 'network',
    title: { hr: 'Mreža', en: 'Network' },
    desc: {
      hr: 'Koja se obitelj adresa koristi pri dohvaćanju s interneta (kućne mreže znaju imati mrtav IPv6).',
      en: 'Which address family is used for outbound requests (home networks often have a dead IPv6).',
    },
    icon: '🌐',
  },
  {
    key: 'ui',
    title: { hr: 'Sučelje', en: 'Interface' },
    desc: {
      hr: 'Jezik ovog sučelja na svim uređajima.',
      en: 'Language of this interface on every device.',
    },
    icon: '🎨',
  },
]

/// Polja: tipovi su text | number | bool | enum | tags | roots | json.
export const FIELDS = {
  'server.friendly_name': {
    label: { hr: 'Naziv servera', en: 'Server name' },
    help: { hr: 'Ono što piše na televizoru kad tražiš server. Prazno → ime računala.', en: 'What appears on the TV when browsing. Empty → host name.' },
    type: 'text',
    placeholder: 'Rustiio',
  },
  'server.bind': {
    label: { hr: 'Adresa za slušanje', en: 'Bind address' },
    help: { hr: '0.0.0.0 znači „sve mrežne kartice" — to je ono što obično želiš.', en: '0.0.0.0 means "all interfaces" — usually what you want.' },
    type: 'text',
    mono: true,
  },
  'server.http_port': {
    label: { hr: 'HTTP port', en: 'HTTP port' },
    help: { hr: 'Port na kojem rade popisi, strimovi i ovo sučelje.', en: 'Port serving listings, streams and this interface.' },
    type: 'number',
    min: 1,
    max: 65535,
  },
  'server.advertise_ip': {
    label: { hr: 'Adresa koja se oglašava', en: 'Advertised address' },
    help: { hr: 'Ostavi prazno da se sama prepozna. Upiši ako računalo ima više mrežnih kartica.', en: 'Leave empty to auto-detect. Set it when the machine has several interfaces.' },
    type: 'text',
    mono: true,
    placeholder: 'auto',
  },
  'server.udn': {
    label: { hr: 'Identitet (UDN)', en: 'Identity (UDN)' },
    help: { hr: 'Po njemu televizori pamte server i pozicije filmova. Promijeni ga samo ako želiš da se server pojavi kao novi.', en: 'TVs remember the server and playback positions by it. Change only to appear as a new server.' },
    type: 'text',
    mono: true,
  },
  'server.max_age_secs': {
    label: { hr: 'Trajanje oglasa', en: 'Advertisement lifetime' },
    help: { hr: 'Koliko sekundi vrijedi SSDP oglas prije ponovnog slanja.', en: 'How long an SSDP advertisement stays valid before repeating.' },
    type: 'number',
    min: 60,
    max: 86400,
    step: 60,
    unit: { hr: 's', en: 's' },
  },
  'server.ssdp': {
    label: { hr: 'Oglašavanje (SSDP)', en: 'Discovery (SSDP)' },
    help: { hr: 'Isključi samo za testiranje — bez toga televizori ne nalaze server sami.', en: 'Turn off only for testing — without it TVs cannot find the server on their own.' },
    type: 'bool',
  },
  'server.log_level': {
    label: { hr: 'Razina zapisnika', en: 'Log level' },
    help: { hr: 'debug i trace pomažu pri traženju problema, ali puno pišu.', en: 'debug and trace help when troubleshooting but are verbose.' },
    type: 'enum',
    options: ['trace', 'debug', 'info', 'warn', 'error'],
  },

  'library.roots': {
    label: { hr: 'Mape s video zapisima', en: 'Video folders' },
    help: {
      hr: 'Rustiio je video server — dodaj mape u kojima ti je video. Klikni „Odaberi" i pregledaj mape na ovom računalu.',
      en: 'Rustiio is a video server — add the folders holding your video. Click "Browse" to look through the folders on this machine.',
    },
    type: 'roots',
  },
  'library.video_extensions': {
    label: { hr: 'Video nastavci', en: 'Video extensions' },
    help: { hr: 'Datoteke s ovim nastavcima ulaze u popis videa.', en: 'Files with these extensions are listed as video.' },
    type: 'tags',
  },
  'library.max_depth': {
    label: { hr: 'Dubina skeniranja', en: 'Scan depth' },
    help: { hr: 'Koliko podmapa se pregledava. 0 = samo zadana mapa.', en: 'How many subfolder levels to walk. 0 = the given folder only.' },
    type: 'number',
    min: 0,
    max: 32,
  },
  'library.views': {
    label: { hr: 'Virtualne mape', en: 'Virtual folders' },
    help: { hr: 'Nedavno dodano, Omiljeno, Po žanru — popisi koji se računaju iz baze.', en: 'Recently added, Favourites, By genre — listings computed from the database.' },
    type: 'bool',
  },
  'library.recent_limit': {
    label: { hr: 'Koliko u „Nedavno"', en: '"Recently added" size' },
    help: { hr: 'Broj stavki u virtualnoj mapi Nedavno dodano.', en: 'Number of items in the Recently added view.' },
    type: 'number',
    min: 1,
    max: 500,
  },
  'library.posters': {
    label: { hr: 'Dohvat postera', en: 'Poster fetching' },
    help: { hr: 'Traži naslovnice (TMDB, Wikipedia, TVmaze…). Isključeno → samo ono što je već u kešu.', en: 'Fetch cover art (TMDB, Wikipedia, TVmaze…). Off → only what is already cached.' },
    type: 'bool',
  },
  'library.watch': {
    label: { hr: 'Praćenje mapa', en: 'Watch folders' },
    help: { hr: 'Nove datoteke se same pojave u popisu, bez ručnog skeniranja.', en: 'New files show up without a manual rescan.' },
    type: 'bool',
  },

  'transcode.enabled': {
    label: { hr: 'Uključeno', en: 'Enabled' },
    help: { hr: 'Kad je isključeno, server šalje datoteku kakva jest (najsigurnije za mrežu, TV mora podnijeti format).', en: 'When off, the file is sent as-is (safest for the network, the TV must support the format).' },
    type: 'bool',
    accent: true,
  },
  'transcode.ffmpeg_path': {
    label: { hr: 'ffmpeg', en: 'ffmpeg' },
    help: { hr: 'Prazno → priloženi ffmpeg uz program, pa PATH.', en: 'Empty → the ffmpeg bundled with the program, then PATH.' },
    type: 'text',
    mono: true,
    placeholder: { hr: 'uz program / PATH', en: 'bundled / PATH' },
  },
  'transcode.ffprobe_path': {
    label: { hr: 'ffprobe', en: 'ffprobe' },
    help: { hr: 'Koristi se za trajanje i metapodatke datoteke.', en: 'Used for duration and file metadata.' },
    type: 'text',
    mono: true,
    placeholder: { hr: 'uz program / PATH', en: 'bundled / PATH' },
  },
  'transcode.hw_accel': {
    label: { hr: 'Hardversko ubrzanje', en: 'Hardware acceleration' },
    help: { hr: 'auto bira najbolje što nađe (NVENC, QuickSync, VideoToolbox…).', en: 'auto picks the best available (NVENC, QuickSync, VideoToolbox…).' },
    type: 'enum',
    options: ['auto', 'nvenc', 'qsv', 'vaapi', 'videotoolbox', 'amf', 'none'],
  },
  'transcode.max_concurrent': {
    label: { hr: 'Paralelni strimovi', en: 'Concurrent streams' },
    help: { hr: 'Koliko istovremenih prekodiranja server dopušta.', en: 'How many transcodes may run at once.' },
    type: 'number',
    min: 1,
    max: 16,
  },
  'transcode.buffer_secs': {
    label: { hr: 'Puffer', en: 'Buffer' },
    help: { hr: 'Sekunde unaprijed koje ffmpeg drži pred TV-om.', en: 'Seconds ffmpeg keeps ahead of the TV.' },
    type: 'number',
    min: 2,
    max: 600,
    unit: { hr: 's', en: 's' },
  },
  'transcode.probe_duration': {
    label: { hr: 'Mjeri trajanje', en: 'Probe duration' },
    help: { hr: 'Isključeno ubrzava popis, ali TV ne vidi trajanje i ne može tražiti „skok na 20. minutu".', en: 'Off speeds up listings, but the TV loses duration and cannot seek.' },
    type: 'bool',
  },

  'profiles.dir': {
    label: { hr: 'Mapa profila', en: 'Profiles folder' },
    help: { hr: 'Prazno → mapa uz config. Tu se spremaju profili naučeni iz stvarnih televizora.', en: 'Empty → folder next to the config. Profiles learned from real TVs are stored here.' },
    type: 'text',
    mono: true,
    placeholder: { hr: 'uz config', en: 'next to config' },
  },
  'profiles.capture': {
    label: { hr: 'Bilježi uređaje', en: 'Capture devices' },
    help: { hr: 'Zapisuje zaglavlja i User-Agent svakog televizora — osnova za novi profil iz stvarnog uređaja.', en: 'Records each TV\'s headers and User-Agent — the basis for a new profile from a real device.' },
    type: 'bool',
  },

  'network.ip_family': {
    label: { hr: 'Obitelj adresa', en: 'Address family' },
    help: { hr: 'ipv4 je najsigurnije; auto koristi IPv6 kad je dostupan.', en: 'ipv4 is safest; auto uses IPv6 when available.' },
    type: 'enum',
    options: ['auto', 'ipv4', 'ipv6'],
  },

  'ui.language': {
    label: { hr: 'Jezik', en: 'Language' },
    help: { hr: 'Vrijedi odmah, bez restarta. auto prati jezik preglednika.', en: 'Applies immediately, no restart. auto follows the browser language.' },
    type: 'enum',
    options: ['auto', 'hr', 'en'],
    hot: true,
  },
}

/// Sekcija kojoj polje pripada (`library.roots` → `library`).
export function sectionOf(path) {
  return path.split('.')[0]
}

/// Opis polja; za nepoznata polja tip se izvede iz vrijednosti.
export function metaOf(path, value) {
  const known = FIELDS[path]
  if (known) return known
  if (Array.isArray(value)) {
    const objects = value.some((item) => item && typeof item === 'object')
    return { type: objects ? 'json' : 'tags', label: { hr: path, en: path }, help: { hr: 'Nije opisano u sučelju — uređuje se kao tekst/JSON.', en: 'Not described in the UI — edited as text/JSON.' } }
  }
  if (typeof value === 'boolean') return { type: 'bool', label: { hr: path, en: path }, help: { hr: '', en: '' } }
  if (typeof value === 'number') return { type: 'number', label: { hr: path, en: path }, help: { hr: '', en: '' } }
  return { type: 'text', label: { hr: path, en: path }, help: { hr: '', en: '' } }
}

/// Redoslijed polja u sekciji = redoslijed iz `FIELDS`, plus sve nepoznato na kraju.
export function fieldsOf(section, config) {
  const value = config?.[section] ?? {}
  const described = Object.keys(FIELDS)
    .filter((path) => sectionOf(path) === section && path.split('.').length === 2)
    .map((path) => path.split('.')[1])
  const rest = Object.keys(value).filter((key) => !described.includes(key))
  return [...described.filter((key) => key in value), ...rest].map((key) => ({
    path: `${section}.${key}`,
    value: value[key],
  }))
}

/// Čitaj vrijednost po putanji `sekcija.polje`.
export function getPath(config, path) {
  const [section, key] = path.split('.')
  return config?.[section]?.[key]
}

/// Vrati **novi** config s postavljenom vrijednošću (bez mutacije).
export function setPath(config, path, next) {
  const [section, key] = path.split('.')
  return { ...config, [section]: { ...(config?.[section] ?? {}), [key]: next } }
}

export const ROOT_KINDS = [
  { id: 'video', label: { hr: 'Video', en: 'Video' } },
  { id: 'audio', label: { hr: 'Glazba', en: 'Audio' } },
  { id: 'image', label: { hr: 'Slike', en: 'Images' } },
]
