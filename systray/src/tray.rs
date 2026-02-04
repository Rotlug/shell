use std::{collections::HashMap, sync::Arc};

use futures::StreamExt;
use tokio::sync::{
    Mutex,
    mpsc::{Receiver, Sender, channel},
};
use zbus::Connection;

use crate::{
    dbus::status_notifier_watcher::{
        StatusNotifierItemRegistered, StatusNotifierItemUnregistered, StatusNotifierWatcherProxy,
    },
    event::Event,
    item::Item,
};

pub struct Tray {
    connection: Connection,
    items: Arc<Mutex<HashMap<String, Arc<Item>>>>,
    event_tx: Sender<Event>,
}

impl Tray {
    pub async fn new() -> crate::Result<(Self, Receiver<Event>)> {
        let connection = zbus::Connection::session().await?;
        let items = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, event_rx) = channel(16);

        // Signals
        let watcher_proxy = StatusNotifierWatcherProxy::builder(&connection)
            .path("/StatusNotifierWatcher")?
            .build()
            .await?;

        let mut item_registered = watcher_proxy
            .receive_status_notifier_item_registered()
            .await?;
        let mut item_unregistered = watcher_proxy
            .receive_status_notifier_item_unregistered()
            .await?;

        let name = connection
            .unique_name()
            .map(|n| n.as_str())
            .unwrap_or_default();

        // Register myself as the host
        watcher_proxy.register_status_notifier_host(name).await?;

        let conn_task = connection.clone();
        let items_task = items.clone();
        let event_tx_task = event_tx.clone();
        tokio::spawn(async move {
            loop {
                let event_tx = event_tx_task.clone();
                let conn_task = conn_task.clone();
                let items_task = items_task.clone();
                tokio::select! {
                    Some(event) = item_registered.next() => {
                        handle_item_registered(event, items_task, conn_task, event_tx).await;
                    }
                    Some(event) = item_unregistered.next() => {
                        handle_item_unregistered(event, items_task, event_tx).await;
                    }
                }
            }
        });

        // Get all system tray items that were present before this method was called, so we
        // can be in sync.
        let addresses = watcher_proxy.registered_status_notifier_items().await?;
        for address in addresses {
            let item = Arc::new(Item::new(connection.clone(), &address, event_tx.clone()).await?);
            items.lock().await.insert(address.clone(), item.clone());
            let _ = event_tx.send(Event::ItemAdded(address, item)).await;
        }

        Ok((
            Self {
                connection,
                items,
                event_tx,
            },
            event_rx,
        ))
    }
}

async fn handle_item_registered(
    event: StatusNotifierItemRegistered,
    items: Arc<Mutex<HashMap<String, Arc<Item>>>>,
    connection: Connection,
    event_tx: Sender<Event>,
) {
    let address = event.args().map(|args| args.arg_1);

    if let Ok(address) = address {
        let item = Arc::new(
            Item::new(connection, address, event_tx.clone())
                .await
                .unwrap(),
        );

        let _ = item.watch_signals().await;

        items.lock().await.insert(address.to_string(), item.clone());
        let _ = event_tx
            .send(Event::ItemAdded(address.to_string(), item))
            .await;
    }
}

async fn handle_item_unregistered(
    event: StatusNotifierItemUnregistered,
    items: Arc<Mutex<HashMap<String, Arc<Item>>>>,
    event_tx: Sender<Event>,
) {
    let address = event.args().map(|args| args.arg_1);

    if let Ok(address) = address {
        items.lock().await.remove(address);
        let _ = event_tx.send(Event::ItemRemoved(address.to_string())).await;
    }
}
