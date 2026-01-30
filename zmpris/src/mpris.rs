use futures::StreamExt;
use std::{collections::HashSet, sync::Arc};
use tokio::{
    sync::{Mutex, broadcast, mpsc},
    task::JoinHandle,
};
use tokio_stream::{StreamMap, wrappers::BroadcastStream};
use zbus::{Connection, fdo};

use crate::{PlaybackStatus, Update, player::Player};

const MPRIS_BUS: &str = "org.mpris.MediaPlayer2";

type Players = StreamMap<Arc<Player>, BroadcastStream<Update>>;

/// This update enum is used to add/remove players from the `StreamMap`,
/// Since the `handle` holds the mutex for the entire time (idk)
/// players can't be added/removed directly from `watch_for_players`
/// therefore that method sends commands to the `handle` to update the `StreamMap`
/// accordingly.
enum PlayerUpdate {
    Added(Arc<Player>),
    Removed(Arc<Player>),
}

/// The `Mpris` struct is used to receive updates from the MPRIS player that is
/// currently playing.
pub struct Mpris {
    active_player: Arc<Mutex<Option<Arc<Player>>>>,
    handle: JoinHandle<()>,
    tx: broadcast::Sender<Update>,
    players_set: Arc<Mutex<HashSet<Arc<Player>>>>,
}

impl Mpris {
    /// Construct a new MPRIS listener
    ///
    /// # Errors
    /// Errors when the dbus connection fails
    pub async fn new() -> crate::Result<Self> {
        let connection = Connection::session().await?;
        let active_player = Arc::new(Mutex::new(None));
        let players_set = Arc::new(Mutex::new(HashSet::new()));
        let (tx, _) = broadcast::channel(16);

        /* Construct Players Map*/
        let mut players: Players = StreamMap::new();
        for name in Self::get_current_players(&connection).await? {
            let player = Player::new(name, &connection).await?;
            let sub = player.subscribe().into();

            players.insert(Arc::new(player), sub);
        }

        /* Start watching for updates */
        let active_player_handle = active_player.clone();
        let tx_handle = tx.clone();
        let mut rx = Self::watch_for_players(connection.clone()).await?;
        let players_set_handle = players_set.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(upd) = players.next() => {
                        let player = upd.0;
                        let Ok(upd) = upd.1 else {
                            continue;
                        };

                        // Only send updates if the player is the one thats
                        // currently playing
                        let active = active_player_handle.lock().await.clone();
                        if let Some(active) = active
                            && player == active {
                                let _ = tx_handle.send(upd.clone());
                        }

                        let mut active = None;
                        if let Update::PlaybackStatus(_) = &upd {
                            /* Find active player */
                            for player in players.keys() {
                                if matches!(player.get_playback_status().await, Ok(PlaybackStatus::Playing)) {
                                    active = Some(player);
                                    let _ = tx_handle.send(upd);
                                    let _ = player.sync().await;
                                    break;
                                }
                            }

                            *active_player_handle.lock().await = active.cloned();
                        }
                    },
                    Some(upd) = rx.recv() => {
                        /* Add/Remove players from the map */
                        match upd {
                            PlayerUpdate::Added(p) => {
                                let sub = p.subscribe();

                                players.insert(p.clone(), sub.into());
                                players_set_handle.lock().await.insert(p.clone());
                            },
                            PlayerUpdate::Removed(p) => {
                                players.remove(&p);
                                players_set_handle.lock().await.remove(&p);
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            active_player,
            handle,
            tx,
            players_set,
        })
    }

    /// Finds all active players and returns their names
    ///
    /// # Errors
    /// errors when the dbus connection fails
    async fn get_current_players(connection: &Connection) -> crate::Result<Vec<String>> {
        let fdo_proxy = fdo::DBusProxy::new(connection).await?;

        let names: Vec<String> = fdo_proxy
            .list_names()
            .await?
            .iter()
            .filter(|name| name.starts_with(MPRIS_BUS))
            .map(std::string::ToString::to_string)
            .collect();

        Ok(names)
    }

    /// Watch for when players are added/removed
    ///
    /// # Errors
    /// errors when the dbus connection fails
    async fn watch_for_players(
        connection: Connection,
    ) -> crate::Result<mpsc::Receiver<PlayerUpdate>> {
        let fdo_proxy = fdo::DBusProxy::new(&connection).await?;

        let (tx, rx) = mpsc::channel(16);

        let mut name_owner_changed = fdo_proxy.receive_name_owner_changed().await?;

        tokio::spawn(async move {
            while let Some(upd) = name_owner_changed.next().await {
                let Ok(args) = upd.args() else {
                    continue;
                };

                if !args.name.starts_with(MPRIS_BUS) {
                    continue;
                }

                let name = args.name.to_string();

                if args.old_owner.is_none() && args.new_owner.is_some() {
                    let Ok(player) = Player::new(name, &connection).await else {
                        continue;
                    };

                    let _ = tx.send(PlayerUpdate::Added(Arc::new(player))).await;
                } else if args.new_owner.is_none() && args.old_owner.is_some() {
                    let Ok(player) = Player::new(name, &connection).await else {
                        continue;
                    };

                    let _ = tx.send(PlayerUpdate::Removed(Arc::new(player))).await;
                }
            }
        });

        Ok(rx)
    }

    /// Subscribe to updates from the ACTIVE player
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Update> {
        self.tx.subscribe()
    }

    /// Returns the active player, or `None` if none is playing.
    #[must_use]
    pub async fn get_active_player(&self) -> Option<Arc<Player>> {
        self.active_player.lock().await.clone()
    }

    /// Returns a `HashSet` of all players currently tracked by the struct
    #[must_use]
    pub async fn get_players(&self) -> HashSet<Arc<Player>> {
        self.players_set.lock().await.clone()
    }

    /// Finds and returns a player, give its `name`.
    /// If no match was found, returns `None`.
    #[must_use]
    pub async fn get_player(&self, name: &str) -> Option<Arc<Player>> {
        self.players_set
            .lock()
            .await
            .iter()
            .find(|p| p.name == name)
            .cloned()
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
