use std::{collections::HashMap, str::FromStr, time::Duration};

use zbus::zvariant::{self, Array};

#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    PlaybackStatus(PlaybackStatus),
    PlaybackRate(f64),
    Position(Duration),
    Metadata(Metadata),
}

/* PlaybackUpdate */
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

impl FromStr for PlaybackStatus {
    type Err = crate::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Playing" => Ok(Self::Playing),
            "Paused" => Ok(Self::Paused),
            "Stopped" => Ok(Self::Stopped),
            _ => Err(crate::Error::ParseError(s.to_string())),
        }
    }
}

/* Metadata */
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub art_uri: Option<String>,
    pub length: Duration,
}

impl TryFrom<HashMap<String, zvariant::OwnedValue>> for Metadata {
    type Error = crate::Error;
    fn try_from(value: HashMap<String, zvariant::OwnedValue>) -> Result<Self, Self::Error> {
        /* Artist */
        let artist = value
            .get("xesam:artist")
            .and_then(|v| v.downcast_ref::<Array>().ok())
            .and_then(|v| v.first().cloned())
            .map(|v| v.to_string());

        /* Title */
        let Some(title) = value.get("xesam:title") else {
            return Err(crate::Error::ParseError(
                "xesam:title not found".to_string(),
            ));
        };

        let title: String = title
            .downcast_ref()
            .map_err(|_| crate::Error::ParseError("xesam:title".to_string()))?;

        /* Length */
        let Some(micros) = value.get("mpris:length") else {
            return Err(crate::Error::ParseError(
                "mpris:length not found".to_string(),
            ));
        };

        let micros: i64 = micros
            .downcast_ref()
            .map_err(|_| crate::Error::ParseError("mpris:length".to_string()))?;

        #[allow(clippy::cast_sign_loss)]
        let length = Duration::from_micros(micros as u64);

        /* Album */
        let album = value
            .get("xesam:album")
            .and_then(|v| v.downcast_ref::<String>().ok())
            .filter(|s| !s.is_empty());

        /* Art URI */
        let art_uri = value
            .get("mpris:artUrl")
            .and_then(|v| v.downcast_ref::<String>().ok())
            .filter(|s| !s.is_empty());

        Ok(Self {
            title,
            artist,
            album,
            art_uri,
            length,
        })
    }
}
