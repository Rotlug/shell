use futures::StreamExt;
use tokio::{
    sync::{Mutex, mpsc::Sender},
    task::JoinHandle,
};
use zbus::Connection;

use crate::{
    dbus::status_notifier_item::StatusNotifierItemProxy, event::Event, menu::Menu, parse_address,
};

#[derive(Debug)]
pub struct Item {
    proxy: StatusNotifierItemProxy<'static>,
    _connection: Connection,
    address: String,
    handles: Mutex<Vec<JoinHandle<()>>>,
    menu: Menu,
    event_tx: Sender<Event>,
}

impl Item {
    pub async fn new(
        connection: Connection,
        address: &str,
        event_tx: Sender<Event>,
    ) -> crate::Result<Self> {
        let (destination, path) = parse_address(address);
        let proxy = StatusNotifierItemProxy::builder(&connection)
            .destination(destination.to_owned())?
            .path(path)?
            .build()
            .await?;

        let menu = Menu::new(
            connection.clone(),
            address.to_owned(),
            destination,
            proxy.menu().await?,
            event_tx.clone(),
        )
        .await?;
        menu.watch_updates().await?;

        Ok(Self {
            proxy,
            _connection: connection,
            address: address.to_string(),
            handles: Mutex::new(Vec::new()),
            menu,
            event_tx,
        })
    }

    pub async fn watch_signals(&self) -> crate::Result<()> {
        let event_tx = self.event_tx.clone();
        let addr = self.address.clone();

        let mut new_icon = self.proxy.receive_new_icon().await?;

        let handle = tokio::spawn(async move {
            while (new_icon.next().await).is_some() {
                let _ = event_tx.send(Event::IconChanged(addr.clone()));
            }
        });

        self.handles.lock().await.push(handle);

        Ok(())
    }

    /// Finds the icon of this item. First trying to get a system icon, then falling back to a
    /// pixmap if available. If both options fail, returns `None`.
    pub async fn icon(&self, size: i32) -> Option<Icon> {
        let system_icon = self.proxy.icon_name().await.ok();
        if let Some(icon) = system_icon
            && !icon.is_empty()
        {
            return Some(Icon::System(icon));
        }

        // System icon not found, trying pixmap
        let pixmap = self
            .proxy
            .icon_pixmap()
            .await
            .ok()
            .filter(|v| !v.is_empty())?;

        let heights: Vec<i32> = pixmap.iter().map(|(_, h, _)| *h).collect();
        let best_height = Icon::best_fit(&heights, size);
        let best_icon = pixmap.iter().find(|(_, h, _)| *h == best_height)?;

        Some(Icon::Pixmap {
            width: best_icon.0,
            height: best_icon.1,
            pixels: best_icon.2.clone(),
        })
    }

    pub const fn menu(&self) -> &Menu {
        &self.menu
    }

    pub async fn title(&self) -> crate::Result<String> {
        let title = self.proxy.title().await?;
        Ok(title)
    }

    pub async fn activate(&self) -> crate::Result<()> {
        self.proxy.activate(0, 0).await?;
        Ok(())
    }
}

impl Drop for Item {
    fn drop(&mut self) {
        let guard = self.handles.try_lock().unwrap();

        for handle in guard.iter() {
            handle.abort();
        }
    }
}

/// Represents an icon in the system tray, which can have two types:
/// * A System icon, which is a string representing an icon from the system's icon pack. e.g.
///   `"discord"` or `"vlc"`.
/// * A Pixmap icon, which is represented using a width, an height, and a `Vec` of bytes in ARGB
///   format.
///
/// When looking for the icon of an `Item`, the System icon takes priority.
#[derive(Debug)]
pub enum Icon {
    System(String),
    Pixmap {
        width: i32,
        height: i32,
        pixels: Vec<u8>,
    },
}

impl Icon {
    /// Find the closest number to `size` in a list of `options`.
    /// This is useful when trying to find the best size to display an icon when multiple sized
    /// pixmaps are available to use.
    #[must_use]
    pub fn best_fit(options: &[i32], size: i32) -> i32 {
        dbg!(options);
        let mut best_fit = (0, u32::MAX);

        for option in options {
            let diff = size.abs_diff(*option);
            if diff < best_fit.1 {
                best_fit.0 = *option;
                best_fit.1 = diff;
            }
        }

        best_fit.0
    }
}
