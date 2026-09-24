# Rustiio — plan razvoja

> Univerzalni DLNA/UPnP media server u Rustu. Jedan binarni fajl, radi svugdje, s web sučeljem i desktop GUI-jem.

**Status:** Faza 0 (temelj) ✅ + Faza 1 (DLNA MVP) ✅ + Faza 2 (profili + transcode) ✅ + Faza 3 (biblioteka) u izradi
**Zadnja izmjena:** 2026-09-24
**Živo:** `Rustiio (box)` radi na 192.168.1.10:8200 (Docker, host mreža, NVENC) — 168 objekata (95 video, 44 audio, 29 mapa), 17 ugrađenih profila.

---

## 1. Cilj i načela

**Cilj:** zamijeniti Serviio (Java, 207 MB RSS, ~25 s start) vlastitim serverom koji je:

- **kompaktan** — jedan binarni fajl (~10 MB), idle RAM < 40 MB, start < 1 s
- **univerzalan** — nijedan TV nije hardkodiran; sve ide kroz **profile uređaja** (TOML) s ugrađenom bazom + "capture" načinom koji profil nauči od pravog uređaja
- **jednostavan za instalaciju** — Linux (.deb/.rpm/.AppImage/systemd), Windows (.msi), macOS (.dmg), Docker; bez Jave, bez runtime ovisnosti
- **lijep i praktičan UI** — isti Svelte UI u browseru (mreža) i u desktop aplikaciji (Tauri webview); taman za telefon
- **ne pada bez ffmpeg-a** — bez ffmpeg-a radi direct play + remux; s ffmpeg-om radi transcode (NVENC/VAAPI/QSV/VideoToolbox/AMF/softver)

**Načela rada:**

1. **Modularno po sektoru** — crate = sektor, fajl = jedna odgovornost. SSDP nikad ne zna za biblioteku; CDS nikad ne zna za ffmpeg.
2. **Nula magije** — protokol (SSDP/UPnP/DIDL) pišemo sami, jer profil-kontrola je cijeli smisao projekta. Ne uzimamo crnu kutiju.
3. **Koegzistencija** — Rustiio i Serviio rade istovremeno na mreži; na TV-u se vide kao dva izvora, pa se parnost dokazuje 1:1.
4. **Mjerljivo** — svaka faza ima acceptance kriterij koji se **mjeri** (VLC/TV, `time`, `/usr/bin/time -l`, curl, tcpdump).
5. **TDD tamo gdje se isplati** — parseri (SSDP, SOAP, DIDL, range) i decision engine imaju testove od prvog dana; UI i glue ne.

---

## 2. Arhitektura

```
                    ┌──────────────────────────────────────────────┐
  TV (Samsung)      │                 rustiio (bin)                │
  TV (Sharp)        │  CLI: run | probe | scan | doctor | init     │
  VLC / Kodi        │                                              │
  telefoni          │  ┌────────────┐   ┌──────────────────────┐   │
      │             │  │ rustiio-   │   │ rustiio-server       │   │
      │  SSDP/UPnP  │  │ ssdp       │   │  axum router         │   │
      ├────────────►│  │  announce  │   │  /rootDesc.xml       │   │
      │             │  │  M-SEARCH  │   │  /*/control (SOAP)   │   │
      │             │  │  probe     │   │  /res/{id} (media)   │   │
      │  HTTP       │  └────────────┘   │  /api/* (UI)         │   │
      └────────────►│                   └──────────┬───────────┘   │
                    │        ┌─────────────────────┼───────────┐   │
                    │        ▼           ▼         ▼           ▼   │
                    │  rustiio-cds  rustiio-http  rustiio-   rustiio-│
                    │  (Browse,     (range,       library   profiles│
                    │   DIDL-Lite)   DLNA hdr,    (scan,    (match, │
                    │                transcode)   index)     caps)  │
                    │                    │                        │
                    │                    ▼  Faza 2                │
                    │            rustiio-transcode (ffmpeg, HW accel)│
                    └──────────────────────────────────────────────┘
                                     ▲
                                     │   isti UI na dva mjesta
                          web (Svelte, /)  +  apps/rustiio-desktop (Tauri)
```

### 2.1 Crate mapa (svaki crate = jedan sektor)

| Crate | Sektor | Odgovornost |
|---|---|---|
| `rustiio-core` | temelj | config (TOML), identitet uređaja (UDN), mrežni pomoćnici, vrijeme/datumi |
| `rustiio-ssdp` | discovery | SSDP socket: NOTIFY alive/byebye, M-SEARCH responder, probe klijent |
| `rustiio-upnp` | protokol | device.xml, SCPD, SOAP envelope, DIDL-Lite, protocolInfo / contentFeatures |
| `rustiio-library` | biblioteka | scan mapa (Faza 1), SQLite indeks + ffprobe metapodaci (Faza 3) |
| `rustiio-cds` | ContentDirectory | Browse/Search → DIDL, sortiranje, paging |
| `rustiio-http` | streaming | byte-range, DLNA headeri, transcode cijev, titlovi |
| `rustiio-profiles` | profili | ugrađena baza profila, matcher (UA/IP/friendlyName), capabilities |
| `rustiio-transcode` | transcode | odluka direct/remux/transcode, ffmpeg naredba, HW accel detekcija |
| `rustiio-server` | orkestracija | axum router, state, REST/WS API, advertise loop |
| `apps/rustiio` | binarni fajl | CLI: `run`, `probe`, `scan`, `doctor`, `init`, `profiles` |
| `apps/rustiio-desktop` | desktop | Tauri v2 šel (Linux/Windows/macOS) — Faza 5 |
| `web/` | UI | Svelte 5 + Vite + TS — Faza 4 |

### 2.2 Ključni tehnički ugovori

**SSDP:** multicast `239.255.255.250:1900`, odgovor unicast s izvornog porta 1900, `CACHE-CONTROL: max-age=1800`, re-announce svakih 900 s, `BOOTID.UPNP.ORG` + `CONFIGID.UPNP.ORG`. Obrađujemo `ssdp:all`, `upnp:rootdevice`, `uuid:*`, `MediaServer:1`, `ContentDirectory:1`, `ConnectionManager:1`.

**SOAP akcije (MVP):**
- ContentDirectory: `Browse`, `Search` (prazan rezultat do Faze 3), `GetSortCapabilities`, `GetSearchCapabilities`, `GetSystemUpdateID`
- ConnectionManager: `GetProtocolInfo`, `GetCurrentConnectionIDs`, `GetCurrentConnectionInfo`

**protocolInfo:** `<mime>:DLNA.ORG_PN=<PN>;DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000`. MP4 dobija PN (`AVC_MP4_MP_HD_1080i_AAC`), MKV/AVI idu bez PN (`video/x-matroska:*`) da TV sam odluči.

**Streaming:** `Accept-Ranges: bytes`, `Content-Range`, `transferMode.dlna.org` (echo zahtjeva, default `Streaming`), `contentFeatures.dlna.org`, titlovi kao `text/srt` resurs + `sec:CaptionInfoEx`.

**Portovi:** HTTP **8200** (konfigurabilno), SSDP 1900. Serviio na .10 drži 8895/23423 — nema sudara.

**ID objekata:** Faza 1 sekvencijalni (`0`, `1`, `2`...), Faza 3 prelazi na SQLite stabilne id-eve (potrebno za watch-state).

---

## 3. Faze

### Faza 0 — Temelj ✅ (ovaj ciklus)

**Zadaci:**
1. Cargo workspace (`crates/*`, `apps/*`), `resolver = "3"`, edition 2024, zajedničke verzije u `[workspace.dependencies]`.
2. `rustiio-core`: config TOML (platformske putanje: `~/Library/Application Support/Rustiio`, `%APPDATA%\Rustiio`, `~/.config/rustiio`), identitet (UDN perzistiran u configu), detekcija LAN IP-a, formatiranje datuma bez chrono.
3. `rustiio-ssdp`: parser/builder poruka (testovi), responder, notifier, probe.
4. `rustiio-upnp`: device.xml, dva SCPD-a, SOAP parse/build/fault, DIDL-Lite builder, protocolInfo tablica.
5. `rustiio-library`: skener mapa → katalog u memoriji (Faza 3: SQLite), povezivanje `.srt` s videom.
6. `rustiio-cds`: `Browse` (Metadata/DirectChildren), sortiranje, paging.
7. `rustiio-http`: byte-range + DLNA headeri + stream tijela.
8. `rustiio-server`: axum router, `/healthz`, `/api/status`, `/api/rescan`, mini landing page.
9. `apps/rustiio`: CLI (`run`, `probe`, `doctor`, `init`).
10. CI (fmt/clippy/test na 3 OS-a), README, rustfmt.

**Acceptance:**
- `cargo test --all` prolazi (SSDP parse, range parse, DIDL snapshot, decision-free).
- `cargo run -p rustiio -- probe` vidi **sebe** i **Serviio na 192.168.1.10**.
- `curl -s localhost:8200/rootDesc.xml | xmllint --noout -` → validan XML.
- VLC → Local Network → vidi "Rustiio (...)"; browse + play MKV/MP4 radi (direct play).
- Oba TV-a vide izvor i reproduciraju film.
- RSS idle < 40 MB (`/usr/bin/time -l` na macOS, `ps -o rss=` na Linuxu).

### Faza 1 — DLNA MVP ✅ (2026-09-24)

- [x] SOAP eventing (`SUBSCRIBE`/`UNSUBSCRIBE`) + pravi `NOTIFY` (`rustiio-server/src/gena.rs`): registar pretplata, obnova po `SID`, `412` za nevaljale zahtjeve, inicijalni event (SEQ 0), ponovni event nakon svakog reskena
- [x] `TimeSeekRange.dlna.org` (seek po vremenu) + lijeno trajanje preko ffprobe-a (`rustiio-library/src/probe.rs`, `rustiio-http/src/time_seek.rs`)
- [x] virtualne kategorije na vrhu stabla: **Video / Nedavno dodano / Muzika / Slike** (`rustiio-cds/src/views.rs`), uključivo/isključivo kroz config (`views`, `recent_limit`)
- [x] Docker slika + `docker-compose.yml` (host mreža zbog SSDP-a) + systemd unit + `rustiio health` za healthcheck
- [x] deploy na .10 (Docker) — Rustiio se oglašava u LAN-u uz router i Mac instancu
- [x] **prvi test na TV-u: Samsung MU6172 vidi izvor i pušta film** (potvrdio korisnik, 2026-09-24)
- [ ] Sharp Aquos — nije još provjeren
- [ ] zapisati što svaki uređaj traži (`tcpdump -i any udp port 1900` + `--log debug`) — mehanizam je spreman (media zahtjevi se logiraju s `range`/`time_seek`/`UA`), zapis slijedi kad se TV spoji

**Acceptance:** TV pušta 1080p H.264 MKV direktno, seek radi, titl se vidi ako ga TV podržava. → *direct play i seek dokazani kroz HTTP/ffprobe; TV potvrda preostaje.*

### Faza 2 — Profili + transcode ✅ (2026-09-24)

- [x] `rustiio-profiles`: TOML baza (17 ugrađenih u `crates/rustiio-profiles/profiles/*.toml`) + matcher po `User-Agent`, `friendlyName`, IP-u
- [x] **Capture način**: server bilježi UA + DLNA zaglavlja po uređaju (`/api/devices`) i generira TOML profil iz stvarnog prometa (`/api/profile/{key}`) — "napravi profil od ovog uređaja" je tako već danas jedna `curl` naredba (dugme u UI-ju dolazi u Fazi 4)
- [x] `rustiio-transcode`: decision engine (kontejner + kodek + rezolucija + kanali → direct/remux/transcode, s razlozima), builder ffmpeg naredbe, HW accel detekcija **testnim enkodiranjem**, `max_concurrent` + red čekanja
- [x] integracija na .10: **NVENC na GTX 1050 Ti** (GPU proslijeđen u kontejner)
- [x] titlovi: srt/vtt kao `res` + `sec:CaptionInfoEx`; burn-in kao opcija profila (uz provjeru da ffmpeg ima `subtitles` filter — ako ga nema, titl ostaje soft umjesto praznog streama)
- [x] remux u MPEG-TS/MP4 (`-c copy`) kad uređaj ne voli kontejner
- [x] seek u transcode streamu: `TimeSeekRange` → ffmpeg s `-ss` + `npt=` u odgovoru

**Acceptance:** HEVC film se pušta na uređaju koji ne podržava HEVC ✅; NVENC radi na .10 ✅; profil se mijenja bez restarta ✅ (`POST /api/profiles/reload`, dokazano na živoj instanci).

**Što ostaje iz ove faze:** Sharp Aquos test; capture dugme u web UI-ju (Faza 4); profili za 3–4 uređaja koja još nisu viđena (dodaju se iz capturea kad se pojave).

### Faza 3 — Biblioteka + metapodaci (u toku)

- [x] SQLite (`rusqlite` bundled) shema: `roots`, `items`, `play_state`, `devices`, FTS5 za pretragu; migracije preko `PRAGMA user_version`
- [x] **stabilni DLNA id-evi**: id dolazi iz baze po putanji (`Catalog::remap_ids`), pa TV koji zapamti `ObjectID` vidi isti film i nakon restarta i nakon reskena
- [x] `ffprobe -print_format json` → trajanje, rezolucija, kodeci, kanali, bitrate — **u bazu**, i natrag u cache pri startu (bez ponovnog mjerenja)
- [x] prepoznavanje serija (`S01E03`, `1x03`, `Sezona 2 Epizoda 5`) → `series`/`season`/`episode` u bazi
- [x] watch-state (uređaj + pozicija, prag 10 s, auto-"odgledano") + `GET /api/continue` ("Nastavi gledati")
- [x] pretraga preko FTS5: `GET /api/search?q=&kind=&limit=` (naslov po prefiksu, putanja po cijeloj riječi, bez dijakritike)
- [x] `GET /api/library` (brojevi po vrsti, serije, shema), `GET/PUT/DELETE /api/playstate/{id}`
- [ ] CDS `Search` akcija preko istog FTS-a (web UI dio Faze 4 ga ionako koristi prvi)
- [ ] `notify` watcher (inotify/FSEvents) → delta scan u sekundi, noćni full scan
- [x] **Posteri i metapodaci, red izvora**: lokalno (`poster.jpg`/`folder.jpg`/`<film>.jpg`) → TMDB → Wikipedia → TVmaze → Cover Art Archive, s kešom na disku (`<config_dir>/art/<id>.<ext>`, atomički upis)
- [x] **Ključ nije obavezan** — provjereno živim pozivima (vidi tablicu niže); `TMDB_API_KEY` u okolini uključuje službeni API, prazno znači keyless
- [x] **Pacing prema hostu** (MusicBrainz traži 1 req/s i vraća 503) + ponovni pokušaji na 429/5xx
- [ ] Poster u DIDL-u (`albumArtURI` + `JPEG_TN`), `/art/{id}` ruta i `poster` stupac u bazi + pozadinsko obogaćivanje
- [ ] titlovi: auto-dohvat (OpenSubtitles/titlovi.com) i madlad HR prijevodi kao modul

**Acceptance:** 10k fajlova indeksirano < 30 s, delta scan < 2 s, poster se vidi u VLC-u i na TV-u, "nastavi gledati" radi. → *indeks, stabilni id-evi, pretraga, watch-state i dohvat postera rade; poster u DIDL-u i delta scan preostaju.*

### Izvori postera — provjereno živim pozivima (2026-09-24)

| izvor | ključ | dokaz (stvarni dohvat) |
|---|---|---|
| lokalno uz datoteku | — | `poster.jpg` → 4096 B u kešu, izvor `local` |
| **TMDB web scrape → media.themoviedb.org** | **ne** | `Sicario` → w500 JPEG, **98 284 B**, izvor `tmdb-web` |
| Wikipedia `pageimages` (`pilicense=any`!) | ne | `Amelie` → `Amélie` poster, **115 210 B** |
| TVmaze `api.tvmaze.com` | ne | `Dark Matter` (2024) → 2000×3000, **1 234 622 B** |
| MusicBrainz + Cover Art Archive | ne | `Nevermind` → 500×500, **102 309 B** |
| TMDB API (`api.themoviedb.org`) | **da** | bez ključa: `401 {"status_code":7,"status_message":"Invalid API key"}` |

Zamke nađene živim testiranjem:

- **TMDB slike same ne traže ključ** (`media.themoviedb.org/t/p/w500/<hash>.jpg`), ali `hash` dolazi s javne stranice pretrage → parser mora rezati HTML po ASCII granicama (TMDB poslužuje i ćirilicu; prva verzija parsera je panicala na `begin <= end`).
- **Wikipedia bez `pilicense=any` vraća `None`** — posteri su "non-free" i inače se ne prikazuju.
- **MusicBrainz dopušta 1 zahtjev/s** i nakon nekoliko brzih poziva vraća **503** (provjereno). Zato `metadata/pacing.rs`: 1,2 s za MusicBrainz/Cover Art, 250 ms za ostale, plus backoff 0,8/2,5/6 s na 429/5xx. Bez toga su živi testovi padali 3 od 4 prolaza; s pacingom **5/5 dvaput zaredom**.

### Faza 4 — Web UI + API (~2 tjedna)

- [ ] REST + WebSocket API (`/api/status`, `/api/scan`, `/api/devices`, `/api/profiles`, `/api/streams`, `/api/settings`, `/api/logs`)
- [ ] Svelte 5 + Vite UI, embeddan u binarni fajl (`rust-embed`), serviran na `/`
- [ ] Dashboard: aktivni streamovi (ko/što/bitrate/transcode ili direct), CPU/GPU, biblioteka, diskovi — **bento grid, draggable/resizable kartice** (kao BSM), tamna tema, mobilni layout
- [ ] Library browser: posteri, filteri (žanr/godina/glumac), pretraga, "pusti na TV" (renderer push)
- [ ] Device manager: lista uređaja, što je tražio, dodjela profila, editor profila s testom
- [ ] Settings: mape, port, titlovi, transcode, korisnici
- [ ] Live logs (tail preko WS), health
- [ ] i18n od početka (hr/en)

**Acceptance:** sve gore se radi iz browsera i s telefona; UI se ne raspada na 380 px; dashboard pokazuje transcode stream u realnom vremenu.

### Faza 5 — Desktop + pakiranje (~1–2 tjedna)

- [ ] Tauri v2 šel: embedded core (bez zasebnog procesa) ili "connect na remote Rustiio", tray ikona, autostart, "otvori UI"
- [ ] Installeri: `.deb`/`.rpm`/`.AppImage`, `.msi`/NSIS, `.dmg`; `brew` formula; Docker `linuxserver`-style slika
- [ ] systemd unit (User=vaha, Restart=on-failure) + Windows service + macOS launchd
- [ ] auto-update (opcionalno, opt-in)
- [ ] cross-compile u CI (x86_64/arm64 za Linux, Windows, macOS)

**Acceptance:** instalacija na .10 u jednoj komandi; na Windowsu dupli klik + tray; na Macu drag u Applications.

### Faza 6 — Moduli koje Serviio ne može (trajno)

- [ ] Renderer/DMR push (Chromecast, DLNA renderer, AirPlay 2 kasnije)
- [ ] per-user pristup + tokeni, remote pristup (HTTPS, bez Caddy-a)
- [ ] plugin/feed sustav (POPIS, IPTV m3u, podcast)
- [ ] notifikacije ("bolji rilz dostupan", "titl preveden", "scan završio") — Telegram/ntfy
- [ ] transcode preseti po uređaju u UI-ju (live izmjena)
- [ ] DLNA eventing (SUBSCRIBE/NOTIFY) + `X_MS_MediaReceiverRegistrar` (Xbox)

### Faza 7 — Hardening

- [ ] fuzz SOAP/DIDL/SSDP parsere, soak test 7 dana, 1000 zahtjeva/s benchmark
- [ ] mjerenje: idle RAM, startup, scan, transcode overhead vs. direktan ffmpeg
- [ ] rollback plan i migracije configa/baze

---

## 4. Verifikacija (kako se dokazuje da radi)

| Sloj | Kako |
|---|---|
| Unit | `cargo test --all` — SSDP parse/build, range, IDL, decision engine, putanje |
| XML | `xmllint --noout` na `/rootDesc.xml`, oba SCPD-a i DIDL iz `Browse` |
| Discovery | `rustiio probe` (mora vidjeti sebe + Serviio), `tcpdump -i en0 udp port 1900` |
| Streaming | `curl -r 100-199 -o /dev/null -w '%{http_code} %{size_download}'`, `ffprobe http://.../res/N/film.mkv` |
| Klijenti | VLC (Mac), Kodi, **oba TV-a**, telefon |
| Resursi | `ps -o rss=`, `time cargo run --release` |
| Parnost | isti film kroz Rustiio i Serviio, uporedi ponašanje (seek, titl, audio) |

**Pravilo:** nijedan zadatak nije gotov dok se ne pokrene na klijentu (VLC ili TV), ne samo u testu.

---

## 5. Rizici i odgovori

| Rizik | Odgovor |
|---|---|
| Proizvođački DLNA kvirki (Samsung traži `contentFeatures`, Sharp traži DIDL redoslijed) | capture način (Faza 2) bilježi što uređaj stvarno traži; profili umjesto nagađanja |
| `bind` na 1900 dok nešto drugo drži port | `SO_REUSEADDR` + `SO_REUSEPORT`; Serviio je na drugom hostu |
| macOS firewall blokira multicast | prvi test na .10 (Linux, bez firewalla); Mac kao klijent |
| Transcode bez HW ubrzanja ubija CPU | detekcija na startu + `max_concurrent` + odbij transcode ako nema HW i profil ne zahtijeva |
| Prevelik scope | YAGNI: Faza 1/2 = samo ono što TV-i trebaju; "svih 200 Serviio profila" se **ne** radi |
| Naziv/brend | `Rustiio` je radni naziv; preimenovanje je jedno `sed` polje u workspaceu |

---

## 6. Otvorena pitanja

1. Zadržati `ffmpeg` kao vanjsku ovisnost ili ga embeddati (GPL/veličina)? — **odluka: vanjski ffmpeg**, s auto-downloadom u Fazi 5.
2. Titlovi: burn-in ili soft? — **po profilu**, default soft (resource).
3. Treba li Rustiio preuzeti i download/pretragu (filmovi/serije) ili ostaje zaseban app? — **ne u Fazi 1–4**, eventualno plugin u Fazi 6.
4. Slika u DLNA (foto galerija) — da, ali tek nakon videa (Faza 3 sekundarno).

---

## 7. Stanje i trenutni fokus

**Faza 0 — završena i provjerena (2026-09-24, macOS, Rust 1.94):**

| provjera | rezultat |
|---|---|
| `cargo test --workspace` | 60 testova prolazi (SSDP parser/builder, range, SOAP args + entiteti, DIDL, config, vrijeme, skener) |
| `cargo clippy --all-targets -- -D warnings` | čisto |
| `xmllint` na `rootDesc.xml`, oba SCPD-a i DIDL iz `Browse` | validno |
| SSDP probe iz druge točke mreže | Rustiio odgovara unicastom s `BOOTID`/`CONFIGID`; u istom scanu nađeni **Serviio (192.168.1.10, DLNADOC/1.50)** i **Samsung TV (192.168.1.100, MediaRenderer + DIAL)** |
| `Browse` (root → Filmovi → film) | DIDL s `res protocolInfo`, `size`, `dc:date`, `sec:CaptionInfoEx` + titl kao `text/srt` resurs |
| `Range: bytes=0-99` | `206 Partial Content` + `Content-Range`; nezadovoljiv range → `416` |
| `ffprobe` preko HTTP (`/res/5/...mkv`) | h264 1280x720 + aac — **direct play radi** |
| `/healthz`, `/api/status`, `/api/rescan` | rade |
| RAM (idle, 2 videa) | **8.9 MB** (Serviio: 207 MB) |

**Faza 1 — završena i provjerena (2026-09-24):**

| provjera | rezultat |
|---|---|
| `cargo test --workspace` | **100 testova** prolazi (dodani: views, time_seek, GENA registar + pravi NOTIFY na slušalicu, health, media plićak) |
| `cargo clippy --workspace --all-targets -- -D warnings` | čisto |
| kategorije na vrhu (`Browse` na `0`) | `Video | Nedavno dodano | Muzika | Slike | Filmovi` (kategorije prve, pa prave mape) |
| `Browse` na `v:video` | vidi filmove iz **svih podmapa** (Serije/S01/Epizoda 1 se pojavi u kategoriji) |
| `BrowseMetadata` na kategoriji | `childCount` točan |
| `TimeSeekRange.dlna.org: npt=00:00:10-` na filmu 20.023 s | `200 OK`, `content-length: 3628017` (pola fajla), `timeseekrange.dlna.org: npt=0:00:10.000-0:00:20.023/0:00:20.023` |
| `TimeSeekRange` bez trajanja (probe isključen) | header se ignorira, ide cijeli fajl (TV se sam prebaci na `Range`) |
| `TimeSeekRange` preko kraja filma | zadnji bajt, nikad prazno tijelo |
| `SUBSCRIBE` bez headera | **412** |
| `SUBSCRIBE` s `CALLBACK` + `NT` | `200`, `SID: uuid:rustiio-…`, `TIMEOUT: Second-300` (poštovan zahtjev TV-a) |
| obnova pretplate (`SUBSCRIBE` + `SID`) | `200` s istim SID-om |
| inicijalni `NOTIFY` | stigao na pravu slušalicu: `SEQ: 0`, `<SystemUpdateID>1</SystemUpdateID>` |
| `POST /api/rescan` | drugi `NOTIFY` s `SEQ: 1` i novim `SystemUpdateID` |
| Docker na .10 | `rustiio Up (healthy)`, 168 objekata (95 video, 44 audio, 29 mapa), `refresh` bez restarta |
| SSDP iz Maca | `192.168.1.10` se oglašava kao `MediaServer:1` + `ContentDirectory:1` + `ConnectionManager:1` |
| RAM | ~9 MB idle (Serviio: 207 MB) |

**Napomena:** `serviio.service` je na .10 trenutno **inactive** — usporedba 1:1 na TV-u čeka da se Serviio upali (ili ne, ako Rustiio odmah radi).

**Faza 3 — prvi dio završen i provjeren (2026-09-24):**

| provjera | rezultat |
|---|---|
| `cargo test --workspace` | **167 testova** prolazi (novi: `series` 6, `store` 17, `library` 3) |
| `cargo clippy --workspace --all-targets -- -D warnings` | čisto |
| baza se stvara uz config | `rustiio.db` (`<config_dir>/rustiio.db`, `RUSTIIO_DB` pregazi), shema 1, WAL |
| DLNA id-evi iz baze | `Browse` vraća `ObjectID` iz SQLite-a (film = `2`), ne redni broj skena |
| **restart servera** | **isti `ObjectID`** (2 → 2) i pozicija na mjestu |
| metapodaci nakon restarta | `media_probed=2` **bez ffprobe-a** (iz baze u cache) |
| `GET /api/search?q=test+film` | pogađa film (FTS5; naslov po prefiksu, putanja po riječi, bez dijakritike) |
| `GET /api/search?q=test&kind=audio` | filter po vrsti radi |
| `PUT/GET /api/playstate/{id}` | pozicija 15000 ms zapisana; VLC i Samsung TV imaju **odvojene** pozicije |
| `GET /api/continue` | nezavršeni film se vraća; nakon 95 % filma nestaje (auto-"odgledano") |

**Sljedeća konkretna akcija:** Faza 3 ostatak — CDS `Search` akcija preko FTS-a, `notify` watcher
(delta scan), TMDB obogaćivanje s poster cacheom.
