use std::time::{SystemTime, UNIX_EPOCH};

use futures::{StreamExt, lock::Mutex};
use tokio::{sync::mpsc::Sender, task::JoinHandle};
use zbus::{
    Connection,
    zvariant::{OwnedObjectPath, OwnedValue, Structure, Value},
};

use crate::{
    dbus::dbusmenu::{DBusMenuProxy, MenuLayout},
    event::Event,
};

/// `Menu` is used for displaying interacting with the right-click menu of a system tray item.
#[derive(Debug)]
pub struct Menu {
    proxy: DBusMenuProxy<'static>,
    connection: Connection,
    handles: Mutex<Vec<JoinHandle<()>>>,
    event_tx: Sender<Event>,
    owner_address: String,
}

impl Menu {
    pub async fn new(
        connection: Connection,
        address: String,
        destination: &str,
        path: OwnedObjectPath,
        event_tx: Sender<Event>,
    ) -> crate::Result<Self> {
        let proxy = DBusMenuProxy::builder(&connection)
            .destination(destination.to_owned())?
            .path(path)?
            .build()
            .await?;

        Ok(Self {
            proxy,
            connection,
            handles: Mutex::new(Vec::new()),
            event_tx,
            owner_address: address,
        })
    }

    /// Spawn the task for updating `items`
    pub async fn watch_updates(&self) -> crate::Result<()> {
        let mut item_properties_updated = self.proxy.receive_items_properties_updated().await?;

        let proxy = self.proxy.clone();

        let event_tx = self.event_tx.clone();
        let owner_address = self.owner_address.clone();
        let handle = tokio::spawn(async move {
            while (item_properties_updated.next().await).is_some() {
                if let Ok((_, layout)) = proxy.get_layout(0, -1, &[]).await
                    && let Ok(items) = Self::parse_layout(layout)
                {
                    let _ = event_tx
                        .send(Event::MenuChanged(owner_address.clone(), items))
                        .await;
                }
            }
        });

        self.handles.lock().await.push(handle);
        Ok(())
    }

    /// Call this method when you are about to open the menu gui. without calling this the menu
    /// will not update with new contents
    pub async fn update_menu(&self) -> crate::Result<()> {
        self.proxy.about_to_show(0).await?;
        Ok(())
    }

    /// Activate an entry by id, this should be called when clicking the item in the menu gui.
    pub async fn activate(&self, id: i32) -> crate::Result<()> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        self.proxy
            .event(id, "clicked", &Value::I32(0), timestamp.as_secs() as u32)
            .await?;

        Ok(())
    }

    pub async fn items(&self) -> crate::Result<Vec<MenuItem>> {
        let (_, layout) = self.proxy.get_layout(0, -1, &[]).await?;

        Self::parse_layout(layout)
    }

    fn parse_layout(layout: MenuLayout) -> crate::Result<Vec<MenuItem>> {
        let mut items = vec![];
        for inner in layout.submenus {
            let item = MenuItem::try_from(&inner)?;
            items.push(item);
        }

        Ok(items)
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        // Abort all handles when the menu item is dropped
        let guard = self.handles.try_lock().unwrap();

        for handle in guard.iter() {
            handle.abort();
        }
    }
}

/// Represents an item in the `Menu`.
#[derive(Default, Debug)]
pub struct MenuItem {
    pub id: i32,
    pub contents: MenuItemContents,
}

// Prase the `OwnedValue` into a menu item
impl TryFrom<&OwnedValue> for MenuItem {
    type Error = crate::Error;

    fn try_from(value: &OwnedValue) -> Result<Self, Self::Error> {
        let structure = value.downcast_ref::<&Structure>()?;
        let mut fields = structure.fields().iter();
        let mut menu_item = Self::default();

        if let Some(Value::I32(id)) = fields.next() {
            menu_item.id = *id;
        }

        if let Some(Value::Dict(dict)) = fields.next() {
            let menu_type = dict
                .get::<&str, &str>(&"type")
                .ok()
                .flatten()
                .unwrap_or_default();

            let label = dict
                .get::<&str, &str>(&"label")?
                .map(|label| label.replace('_', ""));

            let toggle_state = dict
                .get::<&str, i32>(&"toggle-state")
                .ok()
                .flatten()
                .map(ToggleState::from)
                .unwrap_or_default();

            let toggle_type = dict
                .get::<&str, &str>(&"toggle-type")
                .ok()
                .flatten()
                .unwrap_or_default();

            menu_item.contents = match (menu_type, toggle_type, label.clone()) {
                ("separator", _, _) => MenuItemContents::Separator,
                (_, "checkmark", Some(label)) => {
                    MenuItemContents::CheckboxButton(label, toggle_state)
                }
                (_, "radio", Some(label)) => MenuItemContents::RadioButton(label, toggle_state),
                (_, _, Some(label)) => MenuItemContents::Label(label),
                _ => {
                    dbg!(menu_type, toggle_type, label);
                    return Err(crate::Error::MenuItemParse(value.clone()));
                }
            };
        }

        Ok(menu_item)
    }
}

/// Represents the current contents of a `MenuItem`. Possible contents are:
/// * Regular text
/// * Text with radio button
/// * Text with checkbox
/// * Visual Separator
#[derive(Debug, PartialEq, Eq)]
pub enum MenuItemContents {
    Label(String),
    RadioButton(String, ToggleState),
    CheckboxButton(String, ToggleState),
    Separator,
}

impl Default for MenuItemContents {
    fn default() -> Self {
        Self::Label(String::new())
    }
}

/// Represents the current state of a radio button or checkbox
#[derive(Debug, Default, PartialEq, Eq)]
pub enum ToggleState {
    #[default]
    On,
    Off,
    NonInteractive,
}

impl From<i32> for ToggleState {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Off,
            1 => Self::On,
            _ => Self::NonInteractive,
        }
    }
}
