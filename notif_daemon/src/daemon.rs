#![allow(
    clippy::unnecessary_wraps,
    clippy::unused_async,
    clippy::too_many_arguments
)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::sync::mpsc::Sender;
use zbus::{fdo, interface, object_server::SignalEmitter, zvariant};

use crate::{
    event::Event,
    notification::{Action, ClosingReason, Notification, Urgency},
};

#[derive(Debug)]
pub struct Daemon {
    // Generates an incrementing id for each notification
    counter: Arc<Mutex<u32>>,
    event_tx: Sender<Event>,
}

const NOTIFICATION_SPEC_VERSION: &str = "1.2";

impl Daemon {
    pub fn new(event_tx: Sender<Event>) -> Self {
        Self {
            counter: Arc::new(Mutex::new(0)),
            event_tx,
        }
    }
}

#[interface(name = "org.freedesktop.Notifications")]
impl Daemon {
    /// Returns information about this server daemon
    async fn get_server_information(&self) -> fdo::Result<(String, String, String, String)> {
        Ok((
            env!("CARGO_PKG_NAME").to_string(),    // Application name
            env!("CARGO_PKG_AUTHORS").to_string(), // Author/vendor
            env!("CARGO_PKG_VERSION").to_string(), // Version
            NOTIFICATION_SPEC_VERSION.to_string(), // Notification spec version
        ))
    }

    /// Returns which features are supported by this daemon. full list at:
    /// <https://specifications.freedesktop.org/notification/latest/protocol.html#id-1.10.3.2.5>
    async fn get_capabilities(&self) -> fdo::Result<Vec<String>> {
        Ok(["body", "body-markup", "actions"]
            .map(str::to_string)
            .to_vec())
    }

    /// Called when a program sends a notification to the daemon. returns the notification's
    /// generated id.
    async fn notify(
        &self,
        app_name: String,                            // Name of the app
        replaces_id: u32,                            // Id of the notification to replace, if any.
        app_icon: String,                            // Icon
        summary: String,                             // Title
        body: String,                                // Body
        actions: Vec<String>,                        // Buttons on the notification
        hints: HashMap<String, zvariant::Value<'_>>, // Extra metadata
        expire_timeout: i32,                         // Time before it disappears
    ) -> fdo::Result<u32> {
        let id = if replaces_id > 0 {
            replaces_id
        } else {
            let mut counter = self
                .counter
                .lock()
                .map_err(|e| fdo::Error::Failed(format!("Lock poisoned: {e}")))?;

            *counter += 1;
            *counter
        };

        let urgency = hints
            .get("urgency")
            .and_then(|v| v.try_into().ok())
            .map(|v: u8| Urgency::from(u64::from(v)))
            .unwrap_or_default();

        let expire_timeout = if expire_timeout > 0 {
            Some(Duration::from_millis(expire_timeout as u64))
        } else {
            None
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| fdo::Error::Failed(format!("System time error: {e}")))?
            .as_secs();

        let actions = Action::from_vec_strings(actions)
            .map_err(|e| fdo::Error::Failed(format!("Failed to parse actions: {e}")))?;

        let notification = Notification {
            id,
            app_name,
            app_icon,
            summary,
            body,
            expire_timeout,
            urgency,
            timestamp,
            actions,
        };

        self.event_tx
            .send(Event::Incoming(notification))
            .await
            .map_err(|e| fdo::Error::Failed(format!("Failed to send event {e}")))?;

        Ok(id)
    }

    /// Closes a notification by ID
    async fn close_notification(&self, id: u32) -> fdo::Result<()> {
        self.event_tx
            .send(Event::Closed(id))
            .await
            .map_err(|e| fdo::Error::Failed(format!("Failed to send event: {e}")))?;

        Ok(())
    }

    /// Signal emitted when a notification is closed.
    #[zbus(signal)]
    async fn notification_closed(
        signal_emitter: &SignalEmitter<'_>,
        id: u32,
        reason: ClosingReason,
    ) -> zbus::Result<()>;

    /// Signal emitted when a user invokes an action button.
    #[zbus(signal)]
    async fn action_invoked(
        signal_emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: String,
    ) -> zbus::Result<()>;
}
