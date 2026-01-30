use std::time::Duration;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("DBus Error")]
    DBusError(#[from] zbus::Error),

    #[error("DBUS FDO Error")]
    FDOError(#[from] zbus::fdo::Error),

    #[error("Failed to parse string from dbus: '{0}'")]
    ParseError(String),

    #[error("Missing capability to perform this action")]
    MissingCapability,

    #[error("Invalid duration given as an argument: '{0:?}'")]
    InvalidDuration(Duration),
}

pub type Result<T> = std::result::Result<T, Error>;
