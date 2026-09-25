# Rustiio

A DLNA/UPnP media server written in Rust. It turns a folder tree into a Digital Media
Server that TVs, consoles, phones, VLC and Kodi can browse and play from — shipped as a
single binary with the web UI embedded, an optional desktop app, per-device TOML profiles
instead of hardcoded quirks, and on-the-fly transcoding through ffmpeg that travels next
to the binary.

Nothing about a particular client is compiled into the code: what a device can play lives
in a profile file you can read, edit, replace and generate.

## Why it exists

Most media servers accumulate device quirks in source code — an `if` for one vendor's MIME
type, another for a player that refuses a container, a third for a TV that expects a
fabricated response length. Rustiio inverts that. The server knows no devices, only
*profiles*: which containers, codecs, resolutions, bit depths, channel counts and subtitle
modes a client handles, and what to produce when it can't handle the source. Profiles are
plain TOML, can be overridden per installation without recompiling, and can be derived
from observation — Rustiio records what a client actually requested, so an unknown device
becomes a profile instead of a patch.

The second goal is operational: a single self-contained binary, no JVM, no daemon
dependencies, and the same interface in the browser and in the desktop app.

## Features

**Discovery and protocol**

- SSDP `NOTIFY` announcements (alive / byebye) and an `M-SEARCH` responder on
  `239.255.255.250:1900`, coexisting with any other DLNA server on the network
- UPnP `device.xml`, SCPD for `ContentDirectory:1` and `ConnectionManager:1`, SOAP control,
  and GENA eventing (`SUBSCRIBE` / `UNSUBSCRIBE` + `NOTIFY`) so clients are told when the
  library changes instead of holding a stale list
- `rustiio probe` finds other DLNA servers around you, which makes discovery problems
  debuggable from any machine on the LAN

**ContentDirectory**

- Browse and Search to DIDL-Lite, with sorting, paging and advertised search
  capabilities — Search answers TV-style criteria such as `dc:title contains "…"` from a
  local full-text index
- Virtual views on top of the folder tree: *Video*, *Recently added*, *Music*, *Pictures*
- `protocolInfo` / `contentFeatures` computed per object and per profile, including
  `DLNA.ORG_PN`, `DLNA.ORG_OP`, `DLNA.ORG_FLAGS` and `CI` for converted content
- Subtitles advertised through both `res@sec:CaptionInfoEx` and a `sec:CaptionInfoEx`
  element, because clients differ in which of the two they read

**Media serving**

- Byte-range and `TimeSeekRange` handling, so seeking works by bytes for players that ask
  that way and by timestamp for players that only understand time
- Correct `Content-Type`, `transferMode.dlna.org`, `contentFeatures.dlna.org` and
  `Content-Length` behaviour for both progressive files and live transcode streams
- Endpoints: `/res/{id}` for media, `/sub/{id}/{filename}` for subtitles,
  `/tr/{id}` for a transcoded or remuxed stream

**Per-device profiles**

- TOML profiles with `[match]` rules (User-Agent, friendly name, device type, IP), a
  capability block (`[video]`, `[audio]`, `[subtitles]`), a `[transcode]` target and
  optional `[dlna]` details
- 17 profiles are built into the binary — a generic fallback plus common TV, console,
  player and browser classes. Drop a file with the same `id` into the config directory to
  override one, then reload without restarting (`POST /api/profiles/reload`)
- An unmatched client still works on the generic profile; `/api/devices` shows what it
  asked for and `/api/profile/{key}` hands back a generated profile to start from

**On-the-fly transcoding**

- A decision engine chooses one of three outcomes per request — **direct play**,
  **remux** (`-c copy`, when only the container is in the way) or **transcode** — and
  reports its reasons, which is the same data the UI and `/api/decision/{id}` show
- Hardware acceleration: NVIDIA NVENC, VAAPI, Intel Quick Sync, Apple VideoToolbox and AMD
  AMF, with hardware decoding where available, and a software `libx264` fallback. What the
  machine can really do is probed rather than assumed
- Output containers: MPEG-TS, fragmented MP4 or Matroska, selected by profile, including
  the DLNA profile name that fits the chosen codecs
- Transcode sessions are managed: concurrency limit, buffered read-ahead, seeking by
  restarting the encoder at a timestamp, and teardown when the client goes away
- Burn-in subtitles for clients that cannot render soft ones
- A system scan measures the available encoders and recommends a setting; the same probe
  backs `/api/hardware` and the encoder picker in the UI
- Without ffmpeg the server still runs — direct play and remux keep working

**Library**

- Recursive scanning with ffprobe metadata, kept in a SQLite catalog
- Series and episode grouping, title guessed from release names, posters fetched from TMDB
  with an optional API key or from keyless sources when none is set, artwork served from
  `/art/{id}`
- Play-state tracking: continue watching, positions, watched flags
- Filesystem watching (inotify / FSEvents / Windows) for incremental updates, plus manual
  rescan from the CLI and the UI, and a standalone `rustiio posters` pass for artwork

**Interfaces**

- Web UI (Svelte 5) compiled into the binary: dashboard, library, devices and profiles,
  settings, logs; Croatian and English
- REST API over the same state (`/api/status`, `/api/library`, `/api/search`,
  `/api/streams`, `/api/decision/{id}`, `/api/devices`, `/api/profiles`, …) and a live log
  stream over `/ws/logs`
- Desktop app (Tauri 2) that starts the same server in-process and opens the UI in a native
  window
- CLI: `run`, `init`, `doctor`, `health`, `probe`, `posters`,
  `service install|uninstall|status`

## How it works

```
client ── SSDP M-SEARCH ──▶ rustiio-ssdp             announces, answers, or probes
       ◀── LOCATION ───────  /rootDesc.xml           device and service descriptions
client ── SOAP Browse ─────▶ rustiio-cds ──▶ rustiio-library    (catalog + profiles)
       ◀── DIDL-Lite ───────  objects with res URLs
client ── GET /res/{id} ───▶ rustiio-http             byte ranges, DLNA headers
client ── GET /tr/{id} ────▶ rustiio-server ──▶ rustiio-transcode
                                                 decide() → direct / remux / transcode
                                                   └─ ffmpeg → stream on stdout
```

Each crate is one sector and knows nothing about the others' internals — SSDP never sees
the library, ContentDirectory never sees ffmpeg.

| Crate | Sector | Responsibility |
|---|---|---|
| `rustiio-core` | foundation | TOML config, device identity (UDN), network helpers, time |
| `rustiio-ssdp` | discovery | SSDP socket: alive/byebye announces, `M-SEARCH` responder, probe client |
| `rustiio-upnp` | protocol | `device.xml`, SCPD, SOAP envelopes, DIDL-Lite, `protocolInfo` / `contentFeatures` |
| `rustiio-library` | library | folder scanning, SQLite catalog, ffprobe metadata, subtitles, posters, watching |
| `rustiio-cds` | ContentDirectory | Browse/Search → DIDL, views, sorting, paging |
| `rustiio-http` | streaming | byte ranges, TimeSeek, DLNA headers, media files |
| `rustiio-profiles` | devices | profile model, built-in database, matching, capture and profile generation |
| `rustiio-transcode` | transcoding | decision engine, ffmpeg arguments, hardware detection, sessions |
| `rustiio-server` | orchestration | axum router, state, REST/WebSocket API, GENA, embedded web UI |
| `apps/rustiio` | binary | the `rustiio` CLI |
| `apps/rustiio-desktop` | desktop | Tauri 2 window around the same server (own workspace) |

## Platforms

| Target | How |
|---|---|
| Linux (server) | `deploy/native/install.sh` + systemd, Docker/Compose, or bundled release packages (`.deb`, `.rpm`, `.AppImage`) |
| macOS | run the binary, or install it as a launchd user agent (`rustiio service install`); release bundles (`.dmg`) |
| Windows | run the executable, or install it as a Windows service (`rustiio service install`) / Task Scheduler entry; release bundles (`.msi`, NSIS) |
| Desktop app | Tauri 2 on Linux, macOS and Windows — the same server runs in-process |
| Clients | anything DLNA/UPnP: smart TVs, game consoles, phones, VLC, Kodi, browsers |

Pushing a `v*` tag builds macOS (x86_64 and aarch64), Linux x86_64 and Windows x86_64
bundles in CI, each with a static ffmpeg/ffprobe attached as a Tauri sidecar.

## Installation

### Native Linux (script + systemd)

```bash
git clone https://github.com/fvaha/Rustiio.git && cd Rustiio
sudo deploy/native/install.sh
```

The script builds the release binary (or uses a prebuilt one if you pass `BIN=…`), installs
it to `/usr/local/bin/rustiio`, puts `ffmpeg`/`ffprobe` next to it (from `vendor/ffmpeg`, or
copied from the system), writes a config to `/var/lib/rustiio/config.toml` if one does not
exist yet, installs the systemd unit, then enables and starts the service. Re-running it is
safe: it upgrades the binary and restarts the service. Useful overrides: `PREFIX`,
`STATE_DIR`, `SERVICE_USER`, `MEDIA_ROOT`, `BIN`. If `TMDB_API_KEY` is present in the
environment, it is written to `/etc/rustiio/env` with mode 600 and read by the unit — keys
stay out of the repository.

```bash
journalctl -u rustiio -f          # follow the logs
sudo systemctl restart rustiio    # after a config change
rustiio health --quiet            # exits non-zero unless /healthz answers 200
```

### Docker

```bash
mkdir -p deploy/config
cp deploy/config.example.toml deploy/config/config.toml
$EDITOR deploy/config/config.toml      # set advertise_ip and your media roots
docker compose up -d --build
```

`network_mode: host` is not optional: SSDP sends multicast to `239.255.255.250:1900` and
waits for replies, and none of that crosses a Docker bridge. GPU transcoding needs
`nvidia-container-toolkit` on the host; if the GPU is not passed through, Rustiio notices
and falls back to software encoding — transcoding keeps working, it just uses CPU.

### From source

```bash
# the web UI is embedded into the binary at compile time, so build it first
cd web && npm install && npm run build && cd ..

cargo build --release
./target/release/rustiio init       # write a config and print its path
./target/release/rustiio doctor     # check IP, port, ffmpeg, folders
./target/release/rustiio run        # DLNA server + web UI on port 8200
```

Then pick `Rustiio` in a client — in VLC that is *Local Network → Universal Plug'n'Play*.
Web UI: `http://<host>:8200/`, health check: `http://<host>:8200/healthz`.

### Desktop app

The desktop app lives in `apps/rustiio-desktop` as its own Cargo workspace, so the server
workspace stays free of webview dependencies. It compiles the same server in-process and
opens a native window on the web UI. Build it with the Tauri CLI:

```bash
cd apps/rustiio-desktop
cargo tauri build            # needs tauri-cli 2 and the platform webview
```

On Linux that also needs `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`
and `patchelf`. To ship ffmpeg inside the bundle, put static `ffmpeg`/`ffprobe` into
`apps/rustiio-desktop/binaries/` first; the release workflow downloads them and adds them
as sidecars, so a packaged app works without any system ffmpeg.

## Configuration

Configuration is one TOML file — `deploy/config.example.toml` is a complete commented
starting point. The essentials:

```toml
[server]
friendly_name = "Rustiio"
bind = "0.0.0.0"
http_port = 8200
# Address clients should be told to reach. Empty means auto-detect; set it manually
# on a machine with several networks.
advertise_ip = "10.0.0.10"
max_age_secs = 1800
ssdp = true

[library]
views = true
recent_limit = 20

[[library.roots]]
label = "Movies"
path = "/srv/media/movies"
kind = "video"

[transcode]
enabled = true
ffmpeg_path = "ffmpeg"        # an ffmpeg bundled next to the binary wins over this
ffprobe_path = "ffprobe"
hw_accel = "auto"             # auto | none | nvenc | vaapi | qsv | videotoolbox | amf
max_concurrent = 2
buffer_secs = 30
probe_duration = true         # needed for time-based seeking
```

The config directory is `RUSTIIO_CONFIG_DIR` when set, otherwise the platform config
directory (`~/.config/rustiio` or `$XDG_CONFIG_HOME/rustiio` on Linux,
`Library/Application Support/Rustiio` on macOS); the native installer uses `/var/lib/rustiio`.
On first start the server writes its `udn` there — keep that file, because a new identity
makes clients see a different device and forget their state.

### Device profiles

A `profiles/` directory next to `config.toml` may hold additional `*.toml` profiles. A
profile whose `id` matches a built-in one replaces it. Abbreviated example:

```toml
id = "living-room-tv"
name = "Vendor TV (2020+)"
description = "Plays MP4/MKV with H.264 or HEVC up to 4K; wants AC-3 in TS streams."

[match]                        # substring matches, case-insensitive
user_agent = ["VendorTV", "SomeVendor"]
friendly_name = ["[TV]"]
# device_type = ["MediaRenderer"]
# ip = ["10.0.0.50"]           # strongest signal, for manually pinned devices

[video]                        # what the device decodes on its own
containers = ["mp4", "mkv", "ts", "m2ts", "mov"]
codecs = ["h264", "hevc", "mpeg2video", "mpeg4"]
max_width = 3840
max_height = 2160
max_bit_depth = 8              # raise to 10 only if the device really handles it
max_bitrate_kbps = 60000

[audio]
codecs = ["aac", "mp3", "ac3", "eac3"]
max_channels = 6

[subtitles]
mode = "soft"                  # none | soft | burn
formats = ["srt", "vtt"]

[transcode]                    # what to produce when the source does not fit
container = "mpegts"           # mpegts | mp4 | mkv
video_codec = "h264"
audio_codec = "ac3"
audio_channels = 2
max_bitrate_kbps = 8000
max_height = 1080
allow_remux = true             # copy streams when only the container is wrong

[dlna]
op = "01"                      # DLNA.ORG_OP: byte-seek + time-seek
time_seek = true
send_pn = true                 # send DLNA.ORG_PN
```

Every field has a default, so a profile can be five lines long and only state where the
device deviates from a generic DLNA client. To see what the server decides:

```bash
curl -s localhost:8200/api/profiles | jq
curl -s localhost:8200/api/decision/5 | jq
curl -s -H 'User-Agent: ExampleTV/1.0' localhost:8200/api/decision/5 | jq
curl -X POST localhost:8200/api/profiles/reload
```

`/api/devices` lists the clients seen together with what they asked for, and
`/api/profile/{key}` returns a generated profile for a client that matched nothing.

## Technology stack

| Layer | Choice |
|---|---|
| Language | Rust, edition 2024, MSRV 1.85 |
| HTTP server | Axum 0.8, axum-server (optional TLS via rustls) |
| Protocol | SSDP, UPnP SOAP, DIDL-Lite and `protocolInfo` implemented in-tree for full profile control, quick-xml for parsing |
| Storage | SQLite via rusqlite (bundled) |
| Metadata | ffprobe for stream details; TMDB with an optional API key, or keyless providers |
| Transcoding | ffmpeg — NVENC, VAAPI, Quick Sync, VideoToolbox, AMF, `libx264` fallback |
| Web UI | Svelte 5 + Vite, embedded into the binary with rust-embed |
| Desktop | Tauri 2 |
| Packaging | systemd, launchd, Windows service, Docker, Tauri bundles |

## Development

```bash
cargo test --all                          # 348 unit and integration tests
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings

cd web && npm run dev                     # UI dev server, proxies API to :8200
cd web && npm run build                   # → web/dist, embedded by the next cargo build
cd web && npm run check:i18n               # both dictionaries must have the same keys
```

CI runs the three Rust commands on Linux, macOS and Windows. Protocol parsers (SSDP, SOAP,
DIDL, byte ranges, TimeSeek), the transcode decision engine and the profile matcher are
unit-tested; if you change what the server sends a client, expect a test to need updating.

`apps/rustiio-desktop` is a separate workspace, so a root-level `cargo test --all` does not
cover it — build it separately (see above).

## Roadmap

`PLAN.md` holds the detailed development plan. The open items, in short:

- **Profiles**: cover and verify more device classes, and generate a profile from a
  device's own requests without leaving the UI
- **Subtitles**: automatic download and translation providers
- **Metadata**: better title matching for messy release names; filters by genre, year or cast
- **UI**: users and login, per-device transcode presets editable live, "play on another
  device" (DLNA renderer push, later AirPlay)
- **Remote access**: per-user access with tokens, and HTTPS without a reverse proxy
- **Protocol**: `X_MS_MediaReceiverRegistrar` for Xbox clients
- **Modules**: plugin/feed sources (IPTV playlists, podcasts) and notifications on events
- **Packaging**: verify every installer format in CI, a Homebrew formula, macOS
  signing/notarisation, optional auto-update
- **Hardening**: fuzz the SOAP/DIDL/SSDP parsers, long-running soak tests and benchmarks,
  config and database migrations with a rollback plan

## License

Rustiio is free software, released under the **GNU General Public License v3.0 (GPL-3.0)**.
You may use, study, modify and redistribute it freely, including commercially. Any
derivative work — a modified server, or another product built from it — must be distributed
under the same license, with its source code available. The full license text is in
[LICENSE](LICENSE).
