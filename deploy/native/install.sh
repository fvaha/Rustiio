#!/usr/bin/env bash
# Rustiio — native instalacija (bez Dockera, bez ijedne vanjske ovisnosti).
#
#   sudo deploy/native/install.sh                     # build iz izvora + instaliraj
#   sudo BIN=target/release/rustiio deploy/native/install.sh   # predgotov binarni fajl
#   sudo MEDIA_ROOT=/home/vaha/multimedia deploy/native/install.sh
#
# Sto napravi:
#   1. build (osim ako je BIN zadan) — `cargo build --release`
#   2. binarni fajl → /usr/local/bin/rustiio
#   3. prilozi ffmpeg/ffprobe pokraj njega (iz vendor/ffmpeg ili sistemske kopije)
#   4. config → /var/lib/rustiio/config.toml (samo ako ne postoji; postojeci se cuva)
#   5. /etc/systemd/system/rustiio.service → daemon-reload → enable --now
#
# Ponovno pokretanje je sigurno: nadogradi binarni fajl i restartaj servis.
set -euo pipefail

PREFIX=${PREFIX:-/usr/local}
STATE_DIR=${STATE_DIR:-/var/lib/rustiio}
SERVICE_USER=${SERVICE_USER:-${SUDO_USER:-$(id -un)}}
MEDIA_ROOT=${MEDIA_ROOT:-/media}
SRC=${SRC:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)}
BIN=${BIN:-}

log()  { printf '  %s\n' "$*"; }
fail() { printf 'GREŠKA: %s\n' "$*" >&2; exit 1; }

[[ $EUID -eq 0 ]] || fail "pokreni sa sudo (ili kao root)"

echo "Rustiio — native instalacija"
log "izvor:      $SRC"
log "binarni:    $PREFIX/bin/rustiio"
log "state:      $STATE_DIR (config + baza + posteri)"
log "korisnik:   $SERVICE_USER"
log "mediji:     $MEDIA_ROOT"
echo

# --- 1. binarni fajl ---------------------------------------------------------
if [[ -z $BIN ]]; then
    command -v cargo >/dev/null || fail "nema cargo — instaliraj Rust ili zadaj BIN=..."
    log "gradim (cargo build --release)…"
    (cd "$SRC" && cargo build --release --quiet)
    BIN="$SRC/target/release/rustiio"
fi
[[ -f $BIN ]] || fail "binarni fajl ne postoji: $BIN"
install -Dm755 "$BIN" "$PREFIX/bin/rustiio"
log "instaliran $(du -h "$PREFIX/bin/rustiio" | cut -f1) binarni fajl"

# --- 2. ffmpeg u paketu ------------------------------------------------------
# Prilozeni alati zive pokraj nas: `<dir>/ffmpeg`. `rustiio-core/src/tools.rs` ih
# trazi prije PATH-a, pa transcode radi odmah po instalaciji.
for tool in ffmpeg ffprobe; do
    target="$PREFIX/bin/$tool"
    if [[ -x $target ]]; then
        log "$tool: već uz program"
        continue
    fi
    bundled="$SRC/vendor/ffmpeg/$tool"
    if [[ -x $bundled ]]; then
        install -Dm755 "$bundled" "$target"
        log "$tool: iz paketa (vendor/ffmpeg)"
    elif system=$(command -v "$tool"); then
        install -Dm755 "$(readlink -f "$system")" "$target"
        log "$tool: kopiran sa sustava ($system) — u paketu ide statični build"
    else
        log "UPOZORENJE: nema $tool — radi direct play i remux, transcode ne"
    fi
done

# --- 3. config ---------------------------------------------------------------
install -d -o "$SERVICE_USER" -g "$SERVICE_USER" "$STATE_DIR"
config="$STATE_DIR/config.toml"
if [[ -f $config ]]; then
    log "config postoji — ostavljam ga kakav jest ($config)"
else
    template="$SRC/deploy/config.example.toml"
    [[ -f $template ]] || fail "nema predloška: $template"
    sed -e "s#path = \"/media/#path = \"$MEDIA_ROOT/#g" "$template" > "$config"
    chown "$SERVICE_USER:$SERVICE_USER" "$config"
    log "config napisan: $config"
fi

# --- 4. systemd --------------------------------------------------------------
unit=/etc/systemd/system/rustiio.service
install -Dm644 "$SRC/deploy/rustiio.service" "$unit"
sed -i -e "s#^ExecStart=.*#ExecStart=$PREFIX/bin/rustiio run#" \
       -e "s#^ExecStartPost=.*#ExecStartPost=/bin/sh -c 'sleep 2; $PREFIX/bin/rustiio health --quiet'#" \
       -e "s#^User=.*#User=$SERVICE_USER#" \
       -e "s#^Group=.*#Group=$SERVICE_USER#" \
       -e "s#^Environment=RUSTIIO_CONFIG_DIR=.*#Environment=RUSTIIO_CONFIG_DIR=$STATE_DIR#" \
       "$unit"
systemctl daemon-reload
systemctl enable --now rustiio.service >/dev/null
log "systemd: rustiio.service aktivan ($(systemctl is-active rustiio.service))"

# --- 5. provjera -------------------------------------------------------------
sleep 3
if systemctl is-active --quiet rustiio.service; then
    "$PREFIX/bin/rustiio" health --quiet && log "health: OK" || log "UPOZORENJE: health još nije 200 (pogledaj: journalctl -u rustiio -n 30)"
else
    fail "servis nije aktivan — journalctl -u rustiio -n 50"
fi

echo
log "gotovo. Log: journalctl -u rustiio -f"
log "config: $config   (promijeni pa: sudo systemctl restart rustiio)"
