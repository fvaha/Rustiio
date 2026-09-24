//! SSDP sloj — sve sto treba da nas TV uopce primijeti.
//!
//! - [`server`] — odgovara na M-SEARCH i periodicno se oglasava (alive/byebye)
//! - [`probe`] — klijent za `rustiio probe` (nadji DLNA servere u mrezi)
//! - [`message`] — parser i builder poruka (bez I/O, s testovima)

pub mod message;
pub mod probe;
pub mod server;

pub use message::{Message, MessageKind};
pub use probe::{Discovered, discover};
pub use server::{SsdpConfig, SsdpHandle, start};

/// SSDP multicast adresa.
pub const MULTICAST_ADDR: [u8; 4] = [239, 255, 255, 250];
/// SSDP port.
pub const MULTICAST_PORT: u16 = 1900;
