//! REST/WS dijelovi web sučelja — po sektoru, da `routes.rs` ostane samo routing.
//!
//! - [`stats`]: CPU/RAM/diskovi/GPU za dashboard
//! - [`logs`]: zadržane log linije + live tok (`/ws/logs`)
//! - [`settings`]: čitanje i pisanje configa iz browsera
//! - [`fs`]: preglednik mapa (biranje mape s videom bez tipkanja putanje)

pub mod browse;
pub mod fs;
pub mod logs;
pub mod profiles;
pub mod settings;
pub mod stats;
