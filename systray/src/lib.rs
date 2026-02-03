mod dbus;

pub mod tray;

mod error;
pub use error::*;

pub mod item;

pub mod menu;

pub mod event;

/// Utility function to parse the address give by dbus into a proper destination (0) and path (1)
///
/// FIXME: Move me to a separate util module
#[must_use]
pub fn parse_address(address: &str) -> (&str, String) {
    address.split_once('/').map_or_else(
        || (address, String::from("/StatusNotifierItem")),
        |(d, p)| (d, format!("/{p}")),
    )
}
