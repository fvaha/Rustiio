//! Medijski HTTP sloj — ono sto TV zapravo skida s `res` URL-a.
//!
//! Bez ovoga nema seeka: byte-range + DLNA headeri su jedina stvar koja razlikuje
//! "film se pokrene" od "film se pokrene i moze se premotati".

pub mod media;
pub mod range;
pub mod time_seek;

pub use media::{serve_file, serve_node_path};
pub use range::parse_range;
pub use time_seek::{TimeSeek, parse_time_seek};
