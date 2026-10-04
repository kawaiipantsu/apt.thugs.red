pub mod auth;
pub mod config;
pub mod db;
pub mod package;
pub mod preview;
pub mod repository;
pub mod signing;
pub mod tools;
pub mod trust;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_VERSION: &str = env!("XXC_BUILD_VERSION");
pub const ABOUT: &str = "XXC-APTD — a THUGS(red) project by Kawaiipantsu.\nA Debian APT repository daemon, management platform and package browser.\nroot@apt:~$ serve packages, not bullshit.";
pub mod analytics;

pub mod tokens;
