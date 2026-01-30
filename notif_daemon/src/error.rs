use std::sync::mpsc;

use thiserror::Error;

use crate::event::Event;

#[derive(Error, Debug)]
pub enum Error {
    #[error("FDO Error")]
    FDOError(#[from] zbus::fdo::Error),

    #[error("ZBus Error")]
    ZbusError(#[from] zbus::Error),

    #[error("Failed to send event")]
    SendError(#[from] mpsc::SendError<Event>),

    #[error("Failed to parse action list")]
    ActionParseError(Vec<String>),
}

pub type Result<T> = std::result::Result<T, Error>;
