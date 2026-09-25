//! SSDP server: odgovara na `M-SEARCH` i periodicno se oglasava.
//!
//! Zasto ovako rucno: nikakva biblioteka nam ne da kontrolu nad profilima i nad
//! tim koji tocno target odgovaramo, a to je pola kompatibilnosti s TV-ima.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::message::{Message, MessageKind, NotifyParams, ResponseParams, build_notify, build_response};
use crate::{MULTICAST_ADDR, MULTICAST_PORT};

/// Sve sto SSDP sloj treba znati o nama.
#[derive(Debug, Clone)]
pub struct SsdpConfig {
    /// Sučelje na koje se prikljucujemo multicast grupi.
    pub interface: Ipv4Addr,
    /// `http://<ip>:<port>/rootDesc.xml`
    pub location: String,
    /// `SERVER` header.
    pub server: String,
    /// `uuid:...`
    pub udn: String,
    pub device_type: String,
    pub services: Vec<String>,
    pub max_age: u32,
    pub boot_id: u32,
    pub config_id: u32,
}

impl SsdpConfig {
    /// Svi targeti koje oglasavamo: `(NT, USN)`.
    pub fn targets(&self) -> Vec<(String, String)> {
        let usn = |nt: &str| {
            if nt == self.udn { self.udn.clone() } else { format!("{}::{}", self.udn, nt) }
        };
        let mut out = vec![
            ("upnp:rootdevice".to_string(), usn("upnp:rootdevice")),
            (self.udn.clone(), self.udn.clone()),
            (self.device_type.clone(), usn(&self.device_type)),
        ];
        for service in &self.services {
            out.push((service.clone(), usn(service)));
        }
        out
    }

    /// Koje targete odgovaramo na zadani `ST`.
    pub fn matching(&self, st: &str) -> Vec<(String, String)> {
        if st == "ssdp:all" {
            return self.targets();
        }
        self.targets().into_iter().filter(|(nt, _)| nt.eq_ignore_ascii_case(st)).collect()
    }
}

/// Drzak na SSDP zadatke; `stop()` posalje byebye i ugasi ih.
pub struct SsdpHandle {
    cfg: SsdpConfig,
    socket: Arc<UdpSocket>,
    tasks: Vec<JoinHandle<()>>,
}

impl SsdpHandle {
    /// Posalji `ssdp:byebye` (3 puta, UDP je nepouzdan) i ugasi zadatke.
    pub async fn stop(self) {
        for _ in 0..3 {
            if let Err(err) = send_all(&self.socket, &self.cfg, "ssdp:byebye").await {
                debug!(error = %err, "byebye nije poslan");
            }
            sleep(Duration::from_millis(120)).await;
        }
        for task in &self.tasks {
            task.abort();
        }
        info!("SSDP zaustavljen");
    }
}

/// Digni SSDP: bind na 1900, join multicast grupe, alive oglasi, responder.
pub async fn start(cfg: SsdpConfig) -> anyhow::Result<SsdpHandle> {
    let bind: SocketAddr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, MULTICAST_PORT));
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // Da mozemo koegzistirati s drugim DLNA serverima na istom hostu.
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&bind.into())?;
    socket.join_multicast_v4(&Ipv4Addr::from(MULTICAST_ADDR), &cfg.interface)?;
    socket.set_multicast_ttl_v4(4)?;
    socket.set_multicast_loop_v4(true)?;
    socket.set_nonblocking(true)?;

    let socket = Arc::new(UdpSocket::from_std(socket.into())?);

    let announcer = {
        let socket = Arc::clone(&socket);
        let cfg = cfg.clone();
        tokio::spawn(async move {
            // Tri brza alive oglasa na startu (UDP), pa periodicki.
            for _ in 0..3 {
                if let Err(err) = send_all(&socket, &cfg, "ssdp:alive").await {
                    warn!(error = %err, "alive announce nije poslan");
                }
                sleep(Duration::from_millis(200)).await;
            }
            let period = Duration::from_secs((cfg.max_age / 2).max(60) as u64);
            loop {
                sleep(period).await;
                if let Err(err) = send_all(&socket, &cfg, "ssdp:alive").await {
                    warn!(error = %err, "periodicni announce nije poslan");
                }
            }
        })
    };

    let responder = {
        let socket = Arc::clone(&socket);
        let cfg = cfg.clone();
        tokio::spawn(async move { respond_loop(socket, cfg).await })
    };

    info!(
        location = %cfg.location,
        interface = %cfg.interface,
        targets = cfg.targets().len(),
        "SSDP oglasen"
    );

    Ok(SsdpHandle { cfg, socket, tasks: vec![announcer, responder] })
}

async fn respond_loop(socket: Arc<UdpSocket>, cfg: SsdpConfig) {
    let mut buf = vec![0u8; 4096];
    loop {
        let (len, from) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(err) => {
                debug!(error = %err, "recv_from na SSDP socketu");
                sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let text = String::from_utf8_lossy(&buf[..len]).to_string();
        let Some(msg) = Message::parse(&text) else { continue };
        if msg.kind != MessageKind::Search {
            continue;
        }
        let st = msg.search_target().unwrap_or("ssdp:all").to_string();
        let matched = cfg.matching(&st);
        if matched.is_empty() {
            debug!(st = %st, from = %from, "M-SEARCH za target koji ne oglasavamo");
            continue;
        }

        // Spec: odgovor unutar MX sekundi, s malim slucajnim odmakom da 20 TV-a
        // ne opali istovremeno.
        let jitter =
            (SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_micros() as u64).unwrap_or(0)
                % 80)
                + 1;
        sleep(Duration::from_millis(jitter)).await;

        for (nt, usn) in matched {
            let response = build_response(&ResponseParams {
                st: &nt,
                usn: &usn,
                location: &cfg.location,
                server: &cfg.server,
                max_age: cfg.max_age,
                boot_id: cfg.boot_id,
                config_id: cfg.config_id,
            });
            if let Err(err) = socket.send_to(response.as_bytes(), from).await {
                warn!(error = %err, to = %from, "SSDP odgovor nije poslan");
            }
        }
        debug!(st = %st, from = %from, "odgovoreno na M-SEARCH");
    }
}

async fn send_all(socket: &UdpSocket, cfg: &SsdpConfig, nts: &str) -> anyhow::Result<()> {
    let dest: SocketAddr = SocketAddrV4::new(Ipv4Addr::from(MULTICAST_ADDR), MULTICAST_PORT).into();
    for (nt, usn) in cfg.targets() {
        let msg = build_notify(&NotifyParams {
            nts,
            nt: &nt,
            usn: &usn,
            location: &cfg.location,
            server: &cfg.server,
            max_age: cfg.max_age,
            boot_id: cfg.boot_id,
            config_id: cfg.config_id,
        });
        socket.send_to(msg.as_bytes(), dest).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SsdpConfig {
        SsdpConfig {
            interface: Ipv4Addr::new(10, 0, 0, 10),
            location: "http://10.0.0.10:8200/rootDesc.xml".to_string(),
            server: "linux/x86_64 UPnP/1.0 Rustiio/0.1".to_string(),
            udn: "uuid:abc".to_string(),
            device_type: rustiio_core::DEVICE_TYPE.to_string(),
            services: vec![
                rustiio_core::SERVICE_CONTENT_DIRECTORY.to_string(),
                rustiio_core::SERVICE_CONNECTION_MANAGER.to_string(),
            ],
            max_age: 1800,
            boot_id: 1,
            config_id: 1,
        }
    }

    #[test]
    fn targets_include_rootdevice_self_and_services() {
        let targets = cfg().targets();
        let names: Vec<&str> = targets.iter().map(|(nt, _)| nt.as_str()).collect();
        assert!(names.contains(&"upnp:rootdevice"));
        assert!(names.contains(&"uuid:abc"));
        assert!(names.contains(&rustiio_core::DEVICE_TYPE));
        assert!(names.contains(&rustiio_core::SERVICE_CONTENT_DIRECTORY));
        assert_eq!(targets.len(), 5);
        let self_usn = targets.iter().find(|(nt, _)| nt == "uuid:abc").unwrap();
        assert_eq!(self_usn.1, "uuid:abc", "za sebe USN nema :: sufiks");
    }

    #[test]
    fn ssdp_all_matches_everything_and_unknown_matches_nothing() {
        let cfg = cfg();
        assert_eq!(cfg.matching("ssdp:all").len(), 5);
        assert_eq!(cfg.matching("urn:schemas-upnp-org:device:MediaRenderer:1").len(), 0);
        assert_eq!(cfg.matching(rustiio_core::SERVICE_CONTENT_DIRECTORY).len(), 1);
    }
}
