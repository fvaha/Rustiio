// Provjera prijevoda: oba jezika moraju imati iste ključeve, a u komponentama
// ne smije ostati hardkodirani `hr ? '…' : '…'` (jedno mjesto istine = rječnik).
// Pokretanje: npm run check:i18n
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { hr, en } from '../src/lib/strings.js'

function putanje(obj, prefix = '', out = new Set()) {
  for (const [key, value] of Object.entries(obj)) {
    // Ključ s točkom u imenu nikad se ne razriješi (`t()` šeta po segmentima),
    // pa je to tihi bug: tekst se prikaže kao `tr.scan_title`.
    if (key.includes('.')) problem.push(`ključ s točkom u imenu (t() ga ne nađe): ${prefix ? `${prefix}.` : ''}${key}`)
    const path = prefix ? `${prefix}.${key}` : key
    if (value && typeof value === 'object') putanje(value, path, out)
    else out.add(path)
  }
  return out
}

const problem = []
const hrvatski = putanje(hr)
const engleski = putanje(en)

for (const key of hrvatski) if (!engleski.has(key)) problem.push(`nema u en: ${key}`)
for (const key of engleski) if (!hrvatski.has(key)) problem.push(`nema u hr: ${key}`)

function datoteke(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) datoteke(path, out)
    else if (entry.endsWith('.svelte')) out.push(path)
  }
  return out
}

const ternar = /hr\s*\?\s*'|i18n\.lang\s*===\s*'en'\s*\?\s*'|\blang\s*===\s*'en'\s*\?\s*'/
let ternara = 0
for (const path of datoteke(new URL('../src', import.meta.url).pathname)) {
  const linije = readFileSync(path, 'utf8').split('\n')
  linije.forEach((linija, index) => {
    if (ternar.test(linija)) {
      ternara += 1
      problem.push(`${path.replace(/.*\/src\//, 'src/')}:${index + 1} hardkodirani prijevod`)
    }
  })
}

// Svaki `t('ključ')` u kodu mora postojati u oba rječnika i vratiti tekst.
// Dvije tihe greške koje ovo hvata: nepostojeći ključ (t() tada vrati sam tekst
// ključa, npr. „field.en") i ključ koji je objekt (ispisuje se [object Object]).
function nadji(obj, path) {
  let vrijednost = obj
  for (const dio of path.split('.')) {
    if (vrijednost === undefined || vrijednost === null) return undefined
    vrijednost = vrijednost[dio]
  }
  return vrijednost
}

function sviKodovi(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) sviKodovi(path, out)
    else if (/\.(svelte|js)$/.test(entry) && !entry.endsWith('strings.js')) out.push(path)
  }
  return out
}

let poziva = 0
for (const path of sviKodovi(new URL('../src', import.meta.url).pathname)) {
  // Komentari se izbacuju: `t('x')` u komentaru je dokumentacija, ne poziv.
  const izvor = readFileSync(path, 'utf8')
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/(^|[^:'"\\])\/\/[^\n]*/g, '$1')
  const kratko = path.replace(/.*\/src\//, 'src/')
  for (const [, kljuc] of izvor.matchAll(/\bt\(\s*'([^']+)'/g)) {
    poziva += 1
    const hrVrijednost = nadji(hr, kljuc)
    const enVrijednost = nadji(en, kljuc)
    if (hrVrijednost === undefined || enVrijednost === undefined) {
      problem.push(`${kratko}: t('${kljuc}') ne postoji u rječniku (prikazuje se doslovno)`)
    } else if (typeof hrVrijednost === 'object' || typeof enVrijednost === 'object') {
      problem.push(`${kratko}: t('${kljuc}') je objekt, ne tekst (ispisuje se [object Object])`)
    }
  }
}

console.log(`ključeva: hr=${hrvatski.size} en=${engleski.size} | hardkodiranih ternara: ${ternara} | t() poziva provjereno: ${poziva}`)
if (problem.length) {
  console.error('PROBLEMI:\n  ' + problem.join('\n  '))
  process.exit(1)
}
console.log('prijevod je uredan.')
