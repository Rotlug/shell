use std::{hash::Hash, str::FromStr, time::Duration};

use futures::StreamExt;
use tokio::{sync::broadcast, task::JoinHandle};
use zbus::Connection;

use crate::{
    Metadata, PlaybackStatus, Update, generated::player::PlayerProxy, player::position::Position,
};

mod position;

pub enum SeekDirection {
    Forwards,
    Backwards,
}

#[derive(Debug)]
pub struct Player {
    pub name: String,
    proxy: PlayerProxy<'static>,

    tx: broadcast::Sender<Update>,

    handle: JoinHandle<()>,

    // position tracker
    position: Position,
}

impl Player {
    /// Creates a new `Player` object that starts listening to events.
    /// call `subscribe` to receive events for this player.
    ///
    /// # Errors
    /// Errors when the dbus connection fails
    ///
    /// # Panics
    /// Panics when failing to listen to the `Seeked` signal from the player.
    pub async fn new(name: String, connection: &Connection) -> crate::Result<Self> {
        let proxy = PlayerProxy::builder(connection)
            .destination(name.clone())?
            .build()
            .await?;
        let (tx, _) = broadcast::channel(16);

        /* Update task */
        let proxy_handle = proxy.clone();
        let tx_handle = tx.clone();
        let handle = tokio::spawn(async move {
            let mut playback_status = proxy_handle.receive_playback_status_changed().await;
            let mut position = proxy_handle.receive_position_changed().await;
            let mut seeked = proxy_handle
                .receive_seeked()
                .await
                .expect("Failed to listen to seeked signal");
            let mut rate = proxy_handle.receive_rate_changed().await;
            let mut metadata = proxy_handle.receive_metadata_changed().await;

            loop {
                tokio::select! {
                    // Playback Status (Playing/Stopping/Pausing)
                    Some(change) = playback_status.next() => {
                        if let Ok(data) = change.get().await
                        && let Ok(upd) = PlaybackStatus::from_str(&data)
                        {
                            /*
                             * Some players automatically send PlaybackRate 0/1
                             * On start/stop, however, Some dont!
                             */
                            match upd {
                                PlaybackStatus::Playing => {
                                    let _ = tx_handle.send(Update::PlaybackRate(1.0));
                                }
                                PlaybackStatus::Paused | PlaybackStatus::Stopped => {
                                    let _ = tx_handle.send(Update::PlaybackRate(0.0));
                                }
                            }
                            let _ = tx_handle.send(Update::PlaybackStatus(upd));
                        }
                    },
                    // Position
                    Some(change) = position.next() => {
                        if let Ok(data) = change.get().await {
                            #[allow(clippy::cast_sign_loss)]
                            let dur = Duration::from_micros(data as u64);
                            let _ = tx_handle.send(Update::Position(dur));
                        }
                    }
                    // Position (Seeked)
                    Some(change) = seeked.next() => {
                        let Ok(args) = change.args() else {
                            continue;
                        };

                        #[allow(clippy::cast_sign_loss)]
                        let dur = Duration::from_micros(args.value as u64);
                        let _ = tx_handle.send(Update::Position(dur));
                    }
                    // Playback Rate
                    Some(change) = rate.next() => {
                        if let Ok(data) = change.get().await {
                            let _ = tx_handle.send(Update::PlaybackRate(data));
                        }
                    }
                    // Metadata
                    Some(change) = metadata.next() => {
                        if let Ok(data) = change.get().await {
                            let Ok(data) = Metadata::try_from(data) else {
                                continue;
                            };

                            let _ = tx_handle.send(Update::Metadata(data));
                        }
                    }
                }
            }
        });

        let position = Position::new(proxy.clone(), tx.subscribe(), tx.clone()).await?;

        Ok(Self {
            name,
            proxy,
            tx,
            handle,
            position,
        })
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Update> {
        self.tx.subscribe()
    }

    /// Get the current playback status (Playing, paused, stopped).
    ///
    /// # Errors
    /// Errors when a client returns a string that fails to get parsed by
    /// `PlaybackStatus`
    pub async fn get_playback_status(&self) -> crate::Result<PlaybackStatus> {
        PlaybackStatus::from_str(&self.proxy.playback_status().await?)
    }

    /// This method is called from `Mpris` to request updated information when a player
    /// becomes the active one. This can also be used to sync a ui with the current state
    /// of the player.
    ///
    /// # Errors
    /// Errors when the dbus connection fails
    pub async fn sync(&self) -> crate::Result<()> {
        let metadata = self.proxy.metadata().await?;
        if let Ok(metadata) = Metadata::try_from(metadata) {
            let _ = self.tx.send(Update::Metadata(metadata));
        }

        let position = self.position.get_position().await;
        let _ = self.tx.send(Update::Position(position));
        Ok(())
    }

    /* Control Methods */
    async fn play_pause(&self) -> crate::Result<()> {
        if self.proxy.can_pause().await? {
            Ok(self.proxy.play_pause().await?)
        } else {
            Err(crate::Error::MissingCapability)
        }
    }

    async fn stop(&self) -> crate::Result<()> {
        if self.proxy.can_control().await? {
            Ok(self.proxy.stop().await?)
        } else {
            Err(crate::Error::MissingCapability)
        }
    }

    async fn play(&self) -> crate::Result<()> {
        if self.proxy.can_play().await? {
            Ok(self.proxy.play().await?)
        } else {
            Err(crate::Error::MissingCapability)
        }
    }

    async fn seek(&self, duration: Duration, direction: SeekDirection) -> crate::Result<()> {
        #[allow(clippy::cast_possible_wrap)]
        let micros = i64::try_from(duration.as_micros())
            .map_err(|_| crate::Error::InvalidDuration(duration))?;

        let micros = match direction {
            SeekDirection::Forwards => micros,
            SeekDirection::Backwards => -micros,
        };

        if self.proxy.can_seek().await? {
            Ok(self.proxy.seek(micros).await?)
        } else {
            Err(crate::Error::MissingCapability)
        }
    }

    async fn set_position(&self, duration: Duration) -> crate::Result<()> {
        #[allow(clippy::cast_possible_wrap)]
        let current = self.position.get_position().await.as_micros() as i128;
        #[allow(clippy::cast_possible_wrap)]
        let desired = duration.as_micros() as i128;
        let delta = current - desired;

        if self.proxy.can_seek().await? {
            Ok(self
                .proxy
                .seek(i64::try_from(delta).map_err(|_| crate::Error::InvalidDuration(duration))?)
                .await?)
        } else {
            Err(crate::Error::MissingCapability)
        }
    }
}

/* Needed for storing in hashmap */
impl Hash for Player {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Player {}

/* Cleanup */
impl Drop for Player {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
