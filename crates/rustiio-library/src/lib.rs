//! Biblioteka: iz mapa na disku u katalog objekata koje CDS posluzuje.
//!
//! Faza 1 drzi katalog u memoriji (dovoljno za nekoliko tisuca fajlova i trenutni
//! start). Faza 3 ga zamjenjuje SQLite indeksom s ffprobe metapodacima i stabilnim
//! ID-jevima — struktura [`Catalog`] ostaje ista, pa CDS ne treba mijenjati.

pub mod mediainfo;
pub mod probe;
pub mod scan;
pub mod series;
pub mod store;

pub use mediainfo::{AudioStream, MediaInfo, MediaProbe, VideoStream, probe as probe_media};
pub use probe::DurationProbe;
pub use scan::{Catalog, Node, NodeKind, ScanOptions, classify, scan};
pub use series::SeriesInfo;
pub use store::{ItemRow, Position, ScanItem, SearchHit, Store, SyncReport};

/// Ekstenzije titlova koje prepoznajemo (DJELJENO s CDS-om i HTTP slojem).
pub const SUBTITLE_EXTENSIONS: [&str; 5] = ["srt", "vtt", "sub", "ass", "ssa"];

/// Ekstenzije slika koje koristimo za postere/thumbnaileve.
pub const IMAGE_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];
