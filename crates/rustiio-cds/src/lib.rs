//! ContentDirectory (CDS): pretvara katalog u DIDL-Lite koji TV razumije.
//!
//! Sektor je namjerno odvojen od HTTP-a i od biblioteke: ovdje je samo logika
//! "koji objekt, koji filter, koji sort, koja stranica".

pub mod browse;
pub mod views;

pub use browse::{
    BrowseOptions, BrowseOutcome, BrowseRequest, CdsError, MAX_RESULTS, Playback, PlaybackResolver, browse,
    node_to_object, sort_capabilities,
};
pub use views::View;
