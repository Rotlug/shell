use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Connection error: '{0}'")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
