use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("PipeWire error")]
    PipewireError(#[from] pipewire::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
