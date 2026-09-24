//! `/api/stats` — što se događa s računalom: CPU, RAM, diskovi, GPU.
//!
//! Uzorkovanje ide u pozadini (svake 2 s) jer `sysinfo` traži dva mjerenja za
//! postotak CPU-a; handler samo pročita zadnje stanje i ne blokira se.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::Router;
use axum::response::IntoResponse;
use axum::routing::get;
use serde_json::{Value, json};
use sysinfo::{Disks, ProcessesToUpdate, System};

use crate::state::AppState;

/// Koliko često osvježavamo uzorke.
const SAMPLE: Duration = Duration::from_secs(2);

/// Koliko dugo vrijedi izmjereni popis diskova/GPU (oni se ne mijenjaju svake sekunde).
const SLOW_CACHE: Duration = Duration::from_secs(10);

fn system() -> &'static Mutex<System> {
    static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
    SYSTEM.get_or_init(|| Mutex::new(System::new()))
}

/// Pošalji uzorkivač u pozadinu (zove se pri dizanju servera; drugi poziv ne radi ništa).
pub fn start_sampler() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    tokio::spawn(async move {
        loop {
            if let Ok(mut sys) = system().lock() {
                sys.refresh_cpu_usage();
                sys.refresh_memory();
                sys.refresh_processes(ProcessesToUpdate::Some(&[our_pid()]), true);
            }
            tokio::time::sleep(SAMPLE).await;
        }
    });
}

fn our_pid() -> sysinfo::Pid {
    sysinfo::Pid::from_u32(std::process::id())
}

/// Zadnje izmjereno stanje, spremno za JSON.
pub fn snapshot(state: &AppState) -> Value {
    let cpu_usage;
    let cores;
    let brand;
    let total_memory;
    let used_memory;
    let available_memory;
    let total_swap;
    let used_swap;
    let process;
    {
        let Ok(mut sys) = system().lock() else {
            return json!({ "greska": "ne mogu čitati stanje računala" });
        };
        // Prvi poziv nema povijest — osvježi odmah da brojevi ne budu nula.
        if sys.cpus().is_empty() {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
        }
        cpu_usage = sys.global_cpu_usage();
        cores = sys.cpus().len();
        brand = sys.cpus().first().map(|cpu| cpu.brand().to_string()).unwrap_or_default();
        total_memory = sys.total_memory();
        used_memory = sys.used_memory();
        available_memory = sys.available_memory();
        total_swap = sys.total_swap();
        used_swap = sys.used_swap();
        process = sys
            .process(our_pid())
            .map(|process| {
                json!({
                    "pid": process.pid().as_u32(),
                    "memory": process.memory(),
                    "cpu": process.cpu_usage(),
                    "uptime_s": process.run_time(),
                })
            })
            .unwrap_or(Value::Null);
    }

    let load = System::load_average();
    json!({
        "uptime_s": System::uptime(),
        "cpu": {
            "usage": cpu_usage,
            "cores": cores,
            "brand": brand,
            "load": [load.one, load.five, load.fifteen],
        },
        "memory": {
            "total": total_memory,
            "used": used_memory,
            "available": available_memory,
            "swap_total": total_swap,
            "swap_used": used_swap,
        },
        "process": process,
        "disks": cached_slow("disks", disks),
        "gpu": cached_slow("gpu", gpu),
        "knjiznica": {
            "objekata": state.catalog.try_read().map(|catalog| catalog.len()).unwrap_or(0),
            "streamova": state.sessions.active().len(),
            "pretplata": state.gena.len(),
        },
    })
}

/// Diskovi i GPU se mjere rjeđe — rezultat se pamti [`SLOW_CACHE`].
fn cached_slow(key: &str, measure: fn() -> Value) -> Value {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, (Instant, Value)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let Ok(mut cache) = cache.lock() else { return measure() };
    if let Some((at, value)) = cache.get(key) {
        if at.elapsed() < SLOW_CACHE {
            return value.clone();
        }
    }
    let value = measure();
    cache.insert(key.to_string(), (Instant::now(), value.clone()));
    value
}

fn disks() -> Value {
    let disks = Disks::new_with_refreshed_list();
    let list: Vec<Value> = disks
        .iter()
        .filter(|disk| disk.total_space() > 0)
        .map(|disk| {
            let total = disk.total_space();
            let free = disk.available_space();
            let used = total.saturating_sub(free);
            json!({
                "ime": disk.name().to_string_lossy(),
                "montirano": disk.mount_point().to_string_lossy(),
                "fs": disk.file_system().to_string_lossy(),
                "ukupno": total,
                "slobodno": free,
                "iskoristeno": used,
                "posto": if total > 0 { (used as f64 / total as f64 * 100.0).round() } else { 0.0 },
            })
        })
        .collect();
    Value::Array(list)
}

/// GPU preko `nvidia-smi` (nema ga? `null` — transcode tada ide softverski).
fn gpu() -> Value {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Ok(output) = output else { return Value::Null };
    if !output.status.success() {
        return Value::Null;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(first) = text.lines().next() else { return Value::Null };
    let parts: Vec<&str> = first.split(',').map(|part| part.trim()).collect();
    if parts.len() < 5 {
        return Value::Null;
    }
    let number = |index: usize| parts[index].parse::<f64>().unwrap_or(0.0);
    json!({
        "ime": parts[0],
        "zauzetost": number(1),
        "mem_used": number(2) * 1024.0 * 1024.0,
        "mem_total": number(3) * 1024.0 * 1024.0,
        "temp": number(4),
    })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/stats", get(api_stats))
}

async fn api_stats(axum::extract::State(state): axum::extract::State<AppState>) -> impl IntoResponse {
    // Prikupljanje je sinkrono i kratko (mutex + cache), pa ne ide u spawn_blocking.
    axum::Json(snapshot(&state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_returns_null_without_nvidia_smi() {
        // Bez `nvidia-smi` u PATH-u mora vratiti null, ne panic.
        let value = gpu();
        assert!(value.is_null() || value.get("ime").is_some());
    }

    #[test]
    fn disks_are_an_array_with_percentages() {
        let value = disks();
        let list = value.as_array().expect("polje");
        for disk in list {
            let percent = disk.get("posto").and_then(Value::as_f64).expect("posto");
            assert!((0.0..=100.0).contains(&percent));
        }
    }
}
