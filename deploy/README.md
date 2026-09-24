# Deploy

Rustiio je jedan binarni fajl (`.exe` na Windowsu) — nema Jave, nema instalera.
Ovdje su tri načina da ga digneš kao servis.

## 1. Docker (preporučeno na serveru)

```bash
# na serveru, u klonu projekta
mkdir -p deploy/config
cp deploy/config.example.toml deploy/config/config.toml
$EDITOR deploy/config/config.toml          # postavi advertise_ip i roots
docker compose up -d --build
docker compose logs -f rustiio
```

`network_mode: host` u `docker-compose.yml` **nije opcionalan**: SSDP šalje multicast
na 239.255.255.250:1900 i sluša odgovore; kroz Docker bridge TV-i ne vide server.

Provjera da TV-i vide server:

```bash
rustiio probe --wait 4        # s bilo kojeg računala u mreži
```

## 2. systemd (bez Dockera)

```bash
cargo build --release
sudo install -m 0755 target/release/rustiio /usr/local/bin/rustiio
sudo install -m 0644 deploy/rustiio.service /etc/systemd/system/rustiio.service
sudo mkdir -p /home/vaha/.config/rustiio
cp deploy/config.example.toml /home/vaha/.config/rustiio/config.toml
sudo systemctl daemon-reload
sudo systemctl enable --now rustiio
journalctl -u rustiio -f
```

Unit već sadrži `ProtectSystem=strict` + `ProtectHome=read-only`; jedini zapisni put je
`~/.config/rustiio` (tu živi config s UDN-om i eventualna baza).

## 3. macOS / Windows (bez servisa)

macOS: `rustiio run` u pozadini ili `launchd` plist.
Windows: `rustiio.exe run` + Task Scheduler ("At startup"), ili `sc.exe create`
nad `rustiio.exe` (uz `RUSTIIO_CONFIG_DIR` u varijablama okoline).

## Paralelno sa Serviio

Rustiio i Serviio se ne sukobljavaju: drugi DLNA server na istoj mreži je normalna
stvar, TV-i prikažu oba kao dva izvora. Serviio drži port 23424/8895 i svoj SSDP
responder, Rustiio 8200 i svoj — oba odgovaraju na `M-SEARCH` vlastitim `LOCATION`-om.

Ako želiš da se Serviio više ne oglašava dok testiraš: `systemctl stop serviio`.
