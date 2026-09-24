# Rustiio

Univerzalni DLNA/UPnP media server u Rustu — jedan binarni fajl, web sučelje u mreži i desktop GUI,
profili uređaja umjesto hardkodiranih pravila, transcode po potrebi.

Plan razvoja i faze: [`PLAN.md`](PLAN.md).

## Brzi start

```bash
cargo run -p rustiio -- init          # napiše config i pokaže putanju
cargo run -p rustiio -- doctor        # provjeri okolinu (IP, port, ffmpeg, mape)
cargo run -p rustiio -- run           # digne DLNA server + web sučelje
cargo run -p rustiio -- probe         # nađi DLNA servere u mreži (SSDP M-SEARCH)
```

Zatim u VLC-u: **Local Network → Universal Plug'n'Play** → `Rustiio (...)`.

Web sučelje: `http://<ip>:8200/` · Status: `http://<ip>:8200/healthz`

## Struktura

```
crates/rustiio-core        config, identitet uređaja, mreža, vrijeme
crates/rustiio-ssdp        SSDP: announce, M-SEARCH responder, probe
crates/rustiio-upnp        device.xml, SCPD, SOAP, DIDL-Lite, protocolInfo
crates/rustiio-library     skener mapa -> katalog (Faza 3: SQLite + ffprobe)
crates/rustiio-cds         ContentDirectory: Browse/Search -> DIDL
crates/rustiio-http        streaming: byte-range, DLNA headeri, transcode
crates/rustiio-server      axum router, state, REST/WS API
apps/rustiio               CLI binarni fajl
```

## Licenca

MIT
