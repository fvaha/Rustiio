//! Jedan port za dva prometa: DLNA (obični HTTP) i web sučelje (HTTPS).
//!
//! Zašto: televizor ne zna TLS, a preglednik na `http://` piše „not secure".
//! Ovaj sloj pogleda **prvi bajt** veze — `0x16` je TLS i ide na HTTPS
//! slušalicu, sve ostalo je obični HTTP. Zahtjev preglednika (`Accept:
//! text/html`) preusmjerimo na HTTPS pa se upozorenje ni ne pojavi.
//!
//! Veza se samo prekopava (`copy_bidirectional`) — sadržaj se ne dira, pa DLNA
//! put ostaje isti kao da server sluša direktno.

use tokio::net::{TcpListener, TcpStream};

/// Prvi bajt TLS `ClientHello`-a.
const TLS_HANDSHAKE: u8 = 0x16;
/// Koliko zaglavlja pogledamo radi preusmjeravanja preglednika.
const ZAGLAVLJA: usize = 2048;

/// Slušaj na `bind:javni`, pa prekopaj na `127.0.0.1:<http>` ili `<https>`.
pub async fn slusaj(bind: &str, javni: u16, http: u16, https: u16) -> anyhow::Result<()> {
    let listener = TcpListener::bind((bind, javni)).await?;
    tracing::info!(javni, http, https, "front: isti port za HTTP i HTTPS");
    loop {
        let (mut klijent, _) = listener.accept().await?;
        tokio::spawn(async move {
            let mut prvi = [0u8; 1];
            match klijent.peek(&mut prvi).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let tls = prvi[0] == TLS_HANDSHAKE;
            if !tls {
                let mut buf = vec![0u8; ZAGLAVLJA];
                if let Ok(n) = klijent.peek(&mut buf).await {
                    let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                    // Preglednik traži HTML i ne dolazi iz DLNA puta.
                    if head.contains("accept: text/html") && !head.contains("dlna") {
                        if let Some(host) = host_zaglavlja(&head) {
                            preusmjeri(&mut klijent, &host).await;
                            return;
                        }
                    }
                }
            }
            let cilj = if tls { https } else { http };
            match TcpStream::connect(("127.0.0.1", cilj)).await {
                Ok(mut pozadina) => {
                    let _ = tokio::io::copy_bidirectional(&mut klijent, &mut pozadina).await;
                }
                Err(greska) => tracing::debug!(%greska, cilj, "front ne može do pozadine"),
            }
        });
    }
}

/// Vrijednost `Host:` zaglavlja (uz nju preusmjerenje ostaje na istoj adresi).
fn host_zaglavlja(head: &str) -> Option<String> {
    head.lines()
        .find_map(|red| red.strip_prefix("host:"))
        .map(|host| host.trim().to_string())
        .filter(|host| !host.is_empty() && !host.contains('/') && !host.contains(' '))
}

/// `301` na HTTPS inačicu istog porta.
async fn preusmjeri(klijent: &mut TcpStream, host: &str) {
    let odgovor = format!(
        "HTTP/1.1 301 Moved Permanently\r\nlocation: https://{host}/\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    );
    let _ = tokio::io::AsyncWriteExt::write_all(klijent, odgovor.as_bytes()).await;
    let _ = tokio::io::AsyncWriteExt::shutdown(klijent).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_se_uzima_iz_zaglavlja() {
        let head = "get / http/1.1\r\nhost: 10.0.0.10:8200\r\naccept: text/html\r\n\r\n";
        assert_eq!(host_zaglavlja(head).as_deref(), Some("10.0.0.10:8200"));
        assert!(host_zaglavlja("get / http/1.1\r\nhost:\r\n\r\n").is_none());
        assert!(host_zaglavlja("get / http/1.1\r\n\r\n").is_none());
    }
}
