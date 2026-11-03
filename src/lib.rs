//! Ayna — High-performance Rust multi-protocol (AirPlay 2 + Miracast WFD + DLNA/UPnP) receiver for Android TV.

pub mod airplay;
pub mod billing;
pub mod dlna;
pub mod i18n;
pub mod media;
pub mod miracast;
pub mod ui;

#[cfg(target_os = "android")]
pub mod android;
