use thiserror::Error;
use zbus::zvariant::OwnedValue;

#[derive(Error, Debug)]
pub enum Error {
    #[error("DBus Error")]
    DBusError(#[from] zbus::Error),

    #[error("FDO Error")]
    FDOError(#[from] zbus::fdo::Error),

    #[error("ZVarientError")]
    ZVarientError(#[from] zbus::zvariant::Error),

    #[error("Error while parsing menu item: '{0:?}'")]
    MenuItemParse(OwnedValue),
}

pub type Result<T> = std::result::Result<T, Error>;
