//! Aktivne transcode sesije: limit paralelnih, gasenje procesa, popis za UI.
//!
//! Svaka sesija drzi ffmpeg proces i dozvolu iz semafora; kad HTTP tijelo zavrsi
//! (ili klijent prekine), sesija se dropa — proces se ubija, dozvola se vraca.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdout};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{debug, warn};

use crate::decision::Decision;
use crate::ffmpeg::StartRequest;
use crate::hwaccel::HwSupport;

/// Sto je trenutno u pogonu — ide u `/api/streams` i dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveStream {
    pub id: u64,
    pub object_id: String,
    /// Ime uredjaja koji gleda (ili "-").
    pub device: String,
    /// "transcode" / "remux".
    pub mode: String,
    pub target: String,
    pub encoder: String,
    pub started_at: u64,
}

#[derive(Clone)]
pub struct SessionManager {
    ffmpeg_path: String,
    hw: HwSupport,
    limit: Arc<Semaphore>,
    max_concurrent: u32,
    active: Arc<Mutex<HashMap<u64, ActiveStream>>>,
    counter: Arc<AtomicU64>,
}

impl SessionManager {
    pub fn new(ffmpeg_path: String, hw: HwSupport, max_concurrent: u32) -> Self {
        let max_concurrent = max_concurrent.clamp(1, 16);
        Self {
            ffmpeg_path,
            hw,
            limit: Arc::new(Semaphore::new(max_concurrent as usize)),
            max_concurrent,
            active: Arc::new(Mutex::new(HashMap::new())),
            counter: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn hw(&self) -> &HwSupport {
        &self.hw
    }

    pub fn ffmpeg_path(&self) -> &str {
        &self.ffmpeg_path
    }

    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent
    }

    /// Koliko je mjesta ostalo prije nego pocnemo odbijati zahtjeve.
    pub fn available_slots(&self) -> usize {
        self.limit.available_permits()
    }

    pub fn active(&self) -> Vec<ActiveStream> {
        let Ok(active) = self.active.lock() else { return Vec::new() };
        let mut out: Vec<ActiveStream> = active.values().cloned().collect();
        out.sort_by_key(|stream| stream.id);
        out
    }

    /// Pokreni transcode/remux. Vraca `Err` ako je dostignut limit paralelnih poslova.
    pub async fn start(
        &self,
        request: &StartRequest<'_>,
        object_id: &str,
        device: &str,
    ) -> anyhow::Result<Session> {
        let permit = self
            .limit
            .clone()
            .try_acquire_owned()
            .map_err(|_| anyhow!("dostignut limit od {} paralelnih transcodea", self.max_concurrent))?;

        let mut child = crate::ffmpeg::spawn(request)?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("ffmpeg nije dao stdout"))?;

        // ffmpeg log ide u tracing (bez ovoga gubimo razlog zasto stream pukne).
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.trim().is_empty() {
                        warn!(target: "rustiio_transcode", "ffmpeg: {line}");
                    }
                }
            });
        }

        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        let info = ActiveStream {
            id,
            object_id: object_id.to_string(),
            device: device.to_string(),
            mode: mode_label(request.decision),
            target: request.decision.container.clone(),
            encoder: request.decision.video_encoder.clone().unwrap_or_else(|| "copy".to_string()),
            started_at: now_secs(),
        };
        if let Ok(mut active) = self.active.lock() {
            active.insert(id, info.clone());
        }
        debug!(id, object_id, mode = %info.mode, "transcode sesija pocela");

        Ok(Session { id, info, child, stdout: Some(stdout), manager: self.clone(), permit: Some(permit) })
    }

    fn finish(&self, id: u64) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&id);
        }
        debug!(id, "transcode sesija zavrsena");
    }
}

fn mode_label(decision: &Decision) -> String {
    match &decision.mode {
        crate::decision::PlaybackMode::Direct => "direct".to_string(),
        crate::decision::PlaybackMode::Remux => "remux".to_string(),
        crate::decision::PlaybackMode::Transcode { .. } => "transcode".to_string(),
    }
}

/// Jedna aktivna sesija; drop = ubij ffmpeg i oslobodi mjesto.
pub struct Session {
    pub id: u64,
    pub info: ActiveStream,
    pub child: Child,
    /// Izlaz ffmpeg-a; `take_stdout` ga preda HTTP tijelu (proces ostaje vezan na sesiju).
    stdout: Option<ChildStdout>,
    manager: SessionManager,
    permit: Option<OwnedSemaphorePermit>,
}

impl Session {
    pub fn info(&self) -> &ActiveStream {
        &self.info
    }

    /// Preuzmi izlaz ffmpeg-a (moze se zvati samo jednom).
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.stdout.take()
    }

    /// Je li ffmpeg jos ziv (koristi se prije nego sto TV-u kazemo "kraj").
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Prekini odmah (npr. kad uredjaj pusti drugi film).
    pub fn stop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        self.manager.finish(self.id);
        // permit se vraca sam kad se dropa
        self.permit.take();
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::PlaybackMode;
    use crate::ffmpeg::StartRequest;
    use crate::hwaccel::HwAccel;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn soft() -> HwSupport {
        HwSupport {
            available: Vec::new(),
            preferred: HwAccel::None,
            notes: Vec::new(),
            subtitles_filter: false,
            encoder: None,
            threads: 0,
            hardware_decode: true,
        }
    }

    fn remux_decision() -> Decision {
        Decision {
            mode: PlaybackMode::Remux,
            reasons: Vec::new(),
            protocol_info: String::new(),
            mime: "video/mpeg".to_string(),
            container: "mpegts".to_string(),
            video_encoder: None,
            video_bitrate_kbps: None,
            source_size: None,
            max_width: None,
            max_height: None,
            audio_encoder: None,
            audio_channels: None,
            burn_subtitles: false,
            hw: HwAccel::None,
            threads: 0,
            hardware_decode: false,
        }
    }

    /// Generira kratki H.264/AAC MKV; `None` ako ffmpeg nije dostupan.
    fn sample_file(tag: &str) -> Option<PathBuf> {
        let dir = std::env::temp_dir().join(format!("rustiio-session-{tag}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("uzorak.mkv");
        let status = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=10:duration=3",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&file)
            .status();
        matches!(status, Ok(status) if status.success()).then_some(file)
    }

    fn request<'a>(decision: &'a Decision, input: &'a Path) -> StartRequest<'a> {
        StartRequest {
            input,
            source_ext: "mkv",
            decision,
            ffmpeg_path: "ffmpeg",
            start_at_ms: None,
            subtitle: None,
        }
    }

    #[test]
    fn limit_can_not_be_zero() {
        let manager = SessionManager::new("ffmpeg".to_string(), soft(), 0);
        assert_eq!(manager.max_concurrent(), 1);
        assert_eq!(manager.available_slots(), 1);
    }

    #[tokio::test]
    async fn missing_ffmpeg_does_not_wedge_the_server() {
        let manager = SessionManager::new("/nema/ffmpeg".to_string(), soft(), 2);
        let decision = remux_decision();
        let input = PathBuf::from("/media/nema.mkv");

        match manager.start(&request(&decision, &input), "1", "test").await {
            Err(error) => assert!(error.to_string().contains("ne mogu pokrenuti"), "{error}"),
            Ok(mut session) => {
                // Neke platforme prijave gresku tek nakon spawna — bitno je da proces
                // odmah umre i da server ne ostane bez dozvole.
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                assert!(!session.is_running(), "proces bez binarnog fajla mora zavrsiti");
                drop(session);
            }
        }

        assert!(manager.active().is_empty());
        assert_eq!(manager.available_slots(), 2, "dozvola se vratila");
    }

    #[tokio::test]
    async fn real_remux_streams_mpegts_and_releases_the_slot() {
        let Some(file) = sample_file("remux") else {
            eprintln!("ffmpeg nije dostupan — preskacem");
            return;
        };
        let manager = SessionManager::new("ffmpeg".to_string(), soft(), 1);
        let decision = remux_decision();
        let request = request(&decision, &file);

        let mut session = manager.start(&request, "42", "Samsung TV").await.expect("sesija");
        assert_eq!(manager.active().len(), 1);
        assert_eq!(manager.available_slots(), 0);

        // MPEG-TS pocinje s 0x47 na svakih 188 bajtova.
        use tokio::io::AsyncReadExt;
        let mut header = [0u8; 188];
        let read =
            session.take_stdout().expect("stdout dostupan").read(&mut header).await.expect("citanje streama");
        assert!(read > 0, "ffmpeg nije dao podatke");
        assert_eq!(header[0], 0x47, "ocekujem MPEG-TS sync bajt");

        let info = session.info().clone();
        assert_eq!(info.mode, "remux");
        assert_eq!(info.object_id, "42");
        assert_eq!(info.device, "Samsung TV");

        drop(session);
        assert!(manager.active().is_empty(), "sesija se ocistila");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[tokio::test]
    async fn slot_limit_is_enforced_with_a_clear_message() {
        let Some(file) = sample_file("limit") else {
            eprintln!("ffmpeg nije dostupan — preskacem");
            return;
        };
        let manager = SessionManager::new("ffmpeg".to_string(), soft(), 1);
        let decision = remux_decision();
        let request = request(&decision, &file);

        let first = manager.start(&request, "1", "TV A").await.expect("prva sesija");
        let second = manager.start(&request, "2", "TV B").await;
        assert!(second.is_err(), "druga sesija mora biti odbijena dok je limit 1");
        assert!(second.err().unwrap().to_string().contains("limit"));

        drop(first);
        assert_eq!(manager.available_slots(), 1);
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }
}
