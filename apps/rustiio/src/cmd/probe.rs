//! `rustiio probe` — SSDP pretraga mreze (dokaz da nas TV-i vide i sto drugi oglasavaju).

use std::time::Duration;

use crate::cli::ProbeArgs;

pub async fn execute(args: ProbeArgs) -> anyhow::Result<()> {
    println!("\n  Tražim DLNA uređaje: ST={}, čekam {} s (UDP 1900)\n", args.st, args.wait);

    let found = rustiio_ssdp::probe::discover_target(&args.st, Duration::from_secs(args.wait)).await?;

    if found.is_empty() {
        println!("  Nema odgovora.");
        println!("  Provjeri: (1) jesi li na istoj mreži, (2) firewall ne blokira UDP 1900,");
        println!("            (3) probaj veći --wait, (4) da server uopće radi.");
        return Ok(());
    }

    println!("  {:16} {:44} LOCATION / SERVER", "ADRESA", "TARGET");
    println!("  {}", "-".repeat(118));
    for device in &found {
        println!("  {}", device.summary());
    }
    println!("\n  Ukupno {} odgovora\n", found.len());
    Ok(())
}
