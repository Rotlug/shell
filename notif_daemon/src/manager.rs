use std::{collections::HashMap, sync::Arc};

use tokio::sync::{
    Mutex,
    mpsc::{Receiver, Sender, channel},
};
use zbus::{Connection, object_server::SignalEmitter};

use crate::{
    daemon::{Daemon, DaemonSignals},
    event::Event,
    notification::{ClosingReason, Notification},
};

#[derive(Debug)]
pub struct Manager {
    notifications: Arc<Mutex<HashMap<u32, Notification>>>,
    signal_emitter: SignalEmitter<'static>,
    _connection: Connection,
}

impl Manager {
    /// Registers the notification manager and returns a `Receiver` which can be used to listen
    /// to incoming notifications and other events.
    ///
    /// # Errors
    /// Errors when the connection to dbus fails - Which may happen if a different notification
    /// daemon is in use.
    pub async fn new() -> crate::Result<(Self, Receiver<Event>)> {
        // Communication between the `Daemon` and the `Manager`
        let (inner_tx, mut inner_rx) = channel(16);
        // Communication between the `Manager` and the caller
        let (outer_tx, outer_rx) = channel(16);

        let notifications = Arc::new(Mutex::new(HashMap::new()));

        let connection = Connection::session().await?;

        // Daemon
        Self::connect_daemon(&connection, inner_tx.clone()).await?;

        let object_server = connection
            .object_server()
            .interface::<_, Daemon>("/org/freedesktop/Notifications")
            .await?;

        let signal_emitter = object_server.signal_emitter().to_owned();

        let notifications_task = notifications.clone();

        tokio::spawn(async move {
            while let Some(event) = inner_rx.recv().await {
                let mut notifs = notifications_task.lock().await;

                match event {
                    Event::Incoming(ref n) => {
                        notifs.insert(n.id, n.clone());
                    }
                    Event::Closed(id) => {
                        notifs.remove(&id);
                    }
                }

                let _ = outer_tx.send(event).await;
            }
        });

        Ok((
            Self {
                _connection: connection,
                notifications,
                signal_emitter,
            },
            outer_rx,
        ))
    }

    /// Registers as the systems notification daemon.
    ///
    /// # Errors
    /// Errors when the connection fails (for example, when another notification daemon is
    /// running.)
    async fn connect_daemon(connection: &Connection, event_tx: Sender<Event>) -> crate::Result<()> {
        let daemon = Daemon::new(event_tx);

        connection
            .object_server()
            .at("/org/freedesktop/Notifications", daemon)
            .await?;

        connection
            .request_name("org.freedesktop.Notifications")
            .await?;

        Ok(())
    }

    /// Returns a `Notification` given its id, or `None` if none is found.
    pub async fn get_by_id(&self, id: u32) -> Option<Notification> {
        self.notifications.lock().await.get(&id).cloned()
    }

    /// A `Notification` can have a list of actions (corresponding to buttons the user can
    /// click on the client). This method can invoke an action on a notification with a certain
    /// id.
    ///
    /// # Errors
    /// Errors when the dbus signal fails to be emitted
    pub async fn invoke_action(&self, id: u32, action_key: String) -> crate::Result<()> {
        self.signal_emitter.action_invoked(id, action_key).await?;

        Ok(())
    }

    /// Closes a notification with a certain id.
    ///
    /// # Errors
    /// Errors when the dbus signal fails to be emitted
    pub async fn close_notification(
        &self,
        id: u32,
        reason: ClosingReason,
    ) -> crate::Result<Option<Notification>> {
        self.signal_emitter.notification_closed(id, reason).await?;

        Ok(self.notifications.lock().await.remove(&id))
    }
}
