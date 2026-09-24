//! ContentDirectory (CDS): pretvara katalog u DIDL-Lite koji TV razumije.
//!
//! Sektor je namjerno odvojen od HTTP-a i od biblioteke: ovdje je samo logika
//! "koji objekt, koji filter, koji sort, koja stranica".

pub mod browse;

pub use browse::{BrowseOutcome, BrowseRequest, CdsError, MAX_RESULTS, browse, sort_capabilities};
