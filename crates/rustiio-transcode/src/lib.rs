//! Transcode: odluka (direct / remux / transcode), ffmpeg naredba i sesije.
//!
//! Ovaj crate ne zna nista o HTTP-u, DIDL-u ni disku — dobije [`rustiio_library::MediaInfo`]
//! i [`rustiio_profiles::Profile`], a vrati odluku i argumente za ffmpeg.
//!
//! Sektori:
//! - [`decision`] — sto uraditi s fajlom za konkretan uredjaj
//! - [`ffmpeg`]   — gradnja i pokretanje ffmpeg naredbe
//! - [`hwaccel`]  — detekcija NVENC/VAAPI/QSV/VideoToolbox/AMF
//! - [`scan`]     — sken sustava: alati, CPU/GPU, izmjerena brzina enkodera, preporuka
//! - [`session`]  — aktivni transcode streamovi (limit, seek, gasenje)

pub mod decision;
pub mod ffmpeg;
pub mod hwaccel;
pub mod scan;
pub mod session;

pub use decision::{Decision, PlaybackMode, decide};
pub use ffmpeg::{StartRequest, build_args, spawn as spawn_ffmpeg};
pub use hwaccel::{HwAccel, HwSupport, Tuning, detect as detect_hw};
pub use scan::{ScanReport, scan as scan_system};
pub use session::{ActiveStream, Session, SessionManager};
