//! `rustiio health` — provjeri radi li server (koristi ga Docker healthcheck i skripte).
//!
//! Namjerno bez HTTP klijenta u ovisnostima: jedan `GET /healthz` preko golog
//! TCP-a je sve sto treba, a izlazni kod je jedino sto skripte gledaju.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::cli::HealthArgs;

pub async fn execute(args: HealthArgs) -> anyhow::Result<()> {
    let (host, port, path) = parse_url(&args.url).ok_or_else(|| {
        anyhow::anyhow!("ne mogu parsirati URL: {} (ocekujem http://host:port/put)", args.url)
    })?;

    let timeout = Duration::from_secs(args.timeout.max(1));
    match tokio::time::timeout(timeout, probe(&host, port, &path)).await {
        Ok(Ok(200)) => {
            if !args.quiet {
                println!("ok — {} vraca 200", args.url);
            }
            Ok(())
        }
        Ok(Ok(status)) => {
            eprintln!("greska — {} vraca {status}", args.url);
            std::process::exit(1);
        }
        Ok(Err(err)) => {
            eprintln!("greska — {} nedostupan ({err})", args.url);
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("greska — {} nije odgovorio u {timeout:?}", args.url);
            std::process::exit(1);
        }
    }
}

async fn probe(host: &str, port: u16, path: &str) -> std::io::Result<u16> {
    let mut stream = TcpStream::connect((host, port)).await?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHOST: {host}:{port}\r\nUSER-AGENT: rustiio-health\r\nCONNECTION: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let text = String::from_utf8_lossy(&response);
    let status = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    Ok(status)
}

/// `http://127.0.0.1:8200/healthz` -> `("127.0.0.1", 8200, "/healthz")`.
fn parse_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, tail)) => (authority, format!("/{tail}")),
        None => (rest, "/healthz".to_string()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse().ok()?),
        None => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return None;
    }
    Some((host, port, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls_with_and_without_port() {
        assert_eq!(
            parse_url("http://127.0.0.1:8200/healthz"),
            Some(("127.0.0.1".to_string(), 8200, "/healthz".to_string()))
        );
        assert_eq!(
            parse_url("http://192.168.1.10:8200"),
            Some(("192.168.1.10".to_string(), 8200, "/healthz".to_string()))
        );
        assert_eq!(
            parse_url("http://box.local/healthz"),
            Some(("box.local".to_string(), 80, "/healthz".to_string()))
        );
        assert_eq!(parse_url("https://x/healthz"), None);
        assert_eq!(parse_url("http://:8200/healthz"), None);
    }
}
