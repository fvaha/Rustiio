//! REST/WS dijelovi web sučelja — po sektoru, da `routes.rs` ostane samo routing.
//!
//! - [`stats`]: CPU/RAM/diskovi/GPU za dashboard
//! - [`logs`]: zadržane log linije + live tok (`/ws/logs`)
//! - [`settings`]: čitanje i pisanje configa iz browsera
//! - [`fs`]: preglednik mapa (biranje mape s videom bez tipkanja putanje)
//! - [`transcode`]: sken sustava (ffmpeg/CPU/GPU) i automatsko podešavanje transcodea

pub mod browse;
pub mod fs;
pub mod hardware;
pub mod logs;
pub mod profiles;
pub mod settings;
pub mod stats;
pub mod transcode;
