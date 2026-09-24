//! Orkestracijski sloj: spaja SSDP, CDS, HTTP i biblioteku u jedan axum server.
//!
//! Ovaj crate ne zna kako se parsira DIDL ni kako se cita disk — samo povezuje
//! sektore i drzi stanje (config, katalog, identitet).

pub mod gena;
pub mod playback;
pub mod routes;
pub mod state;

pub use gena::Registry as GenaRegistry;
pub use playback::PlaybackEngine;
pub use routes::router;
pub use state::AppState;
