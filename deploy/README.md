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

## Profili uređaja

Uz `config.toml`, u istom direktoriju (`RUSTIIO_CONFIG_DIR`) može stajati `profiles/` s
dodatnim `*.toml` profilima. Profil s istim `id` pregazi ugrađeni. Reload bez restarta:

```bash
curl -X POST localhost:8200/api/profiles/reload
curl -s localhost:8200/api/profiles | jq
curl -s -H 'User-Agent: SEC_HHP_[TV]UE55MU6172/1.0' localhost:8200/api/decision/5 | jq
```

Ako TV nije prepoznat, pusti ga da nas zamoli bilo što, pa pogledaj `/api/devices`
(recept iz capturea) i uzmi generirani TOML s `/api/profile/<key>`.

## GPU transcode

`docker-compose.yml` već traži nvidia GPU (`deploy.resources.reservations.devices`).
Na hostu treba `nvidia-container-toolkit`; provjera da ffmpeg u kontejneru vidi NVENC:

```bash
docker exec rustiio ffmpeg -hide_banner -encoders | grep nvenc
curl -s localhost:8200/api/status | jq '.transcode'
```

Ako GPU nije proslijeđen, Rustiio to sam primijeti i pada na softverski `libx264`
(vidi `notes` u `/api/status`) — transcode i dalje radi, samo troši CPU.

## Paralelno sa Serviio

Rustiio i Serviio se ne sukobljavaju: drugi DLNA server na istoj mreži je normalna
stvar, TV-i prikažu oba kao dva izvora. Serviio drži port 23424/8895 i svoj SSDP
responder, Rustiio 8200 i svoj — oba odgovaraju na `M-SEARCH` vlastitim `LOCATION`-om.

Ako želiš da se Serviio više ne oglašava dok testiraš: `systemctl stop serviio`.
