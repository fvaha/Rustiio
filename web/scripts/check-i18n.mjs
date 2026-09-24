// Provjera prijevoda: oba jezika moraju imati iste ključeve, a u komponentama
// ne smije ostati hardkodirani `hr ? '…' : '…'` (jedno mjesto istine = rječnik).
// Pokretanje: npm run check:i18n
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { hr, en } from '../src/lib/strings.js'

function putanje(obj, prefix = '', out = new Set()) {
  for (const [key, value] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${key}` : key
    if (value && typeof value === 'object') putanje(value, path, out)
    else out.add(path)
  }
  return out
}

const hrvatski = putanje(hr)
const engleski = putanje(en)
const problem = []

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

const ternar = /hr\s*\?\s*'|i18n\.lang\s*===\s*'en'\s*\?\s*'/
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

console.log(`ključeva: hr=${hrvatski.size} en=${engleski.size} | hardkodiranih ternara: ${ternara}`)
if (problem.length) {
  console.error('PROBLEMI:\n  ' + problem.join('\n  '))
  process.exit(1)
}
console.log('prijevod je uredan.')
