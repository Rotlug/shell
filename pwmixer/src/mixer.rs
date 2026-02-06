use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    thread::{self, JoinHandle},
};

use libspa::utils::dict::DictRef;
use pipewire::{
    context::ContextRc,
    main_loop::{MainLoop, MainLoopRc},
    registry::{GlobalObject, Registry},
    types::ObjectType,
};
use tokio::sync::mpsc;

use crate::{
    Command, Event, MetadataChange, ObjectId,
    device::{self, DeviceHints, DeviceTracker},
    metadata::{self, MetadataControl, MetadataTracker},
    node::{self, NodeProps, NodeTracker},
};

/// Responsible for the setup and destruction of the pipewire main loop thread.
pub struct Mixer {
    pub handle: JoinHandle<crate::Result<()>>,
}

type NodeMap = HashMap<ObjectId, NodeTracker>;
type DeviceMap = HashMap<ObjectId, DeviceTracker>;

impl Mixer {
    #[must_use]
    pub fn new(mut command_rx: mpsc::Receiver<Command>) -> (Self, mpsc::Receiver<Event>) {
        let (event_tx, event_rx) = mpsc::channel(16);

        // pipewire channel is a non blocking version of mpsc::channel
        let (pipewire_tx, pipewire_rx) = pipewire::channel::channel();

        /* --- Pipewire Thread --- */
        let handle = Self::spawn_pipewire_thread(event_tx.clone(), pipewire_rx);

        /* This task if for sending the commands from the tokio channel to the pipewire
         * channel, so that the user of this struct does not have to use the pipewire
         * specific channel type.
         */
        tokio::spawn(async move {
            while let Some(command) = command_rx.recv().await {
                let _ = pipewire_tx.send(command);
            }
        });

        (Self { handle }, event_rx)
    }

    fn spawn_pipewire_thread(
        event_tx: mpsc::Sender<Event>,
        pipewire_rx: pipewire::channel::Receiver<Command>,
    ) -> JoinHandle<crate::Result<()>> {
        thread::spawn(move || -> crate::Result<()> {
            let mainloop = MainLoopRc::new(None)?;
            let context = ContextRc::new(&mainloop, None)?;
            let core = context.connect_rc(None)?;
            let registry = core.get_registry_rc()?;

            let registry_clone = registry.clone();

            let nodes: Rc<RefCell<NodeMap>> = Rc::new(RefCell::new(HashMap::new()));
            let devices: Rc<RefCell<DeviceMap>> = Rc::new(RefCell::new(HashMap::new()));
            let metadata: Rc<RefCell<Option<MetadataTracker>>> = Rc::new(RefCell::new(None));

            let nodes_event = nodes.clone();
            let devices_event = devices.clone();
            let metadata_event = metadata.clone();
            let event_tx_event = event_tx.clone();

            let event_tx_meta = event_tx.clone();
            let nodes_meta = nodes.clone();

            let nodes_cmd = nodes.clone();
            let mainloop_cmd = mainloop.clone();
            let metadata_cmd = metadata;
            let devices_cmd = devices.clone();

            // Used for communication between the `MetadataTracker` and this thread.
            let (meta_tx, meta_rx) = pipewire::channel::channel();

            // Event listener
            let _event_listener = registry
                .add_listener_local()
                .global(move |object| {
                    Self::handle_new_object(
                        object,
                        &nodes_event,
                        &devices_event,
                        &metadata_event,
                        &event_tx_event,
                        &meta_tx.clone(),
                        &registry_clone,
                    );
                })
                .global_remove(move |id| {
                    Self::handle_object_deletion(ObjectId(id), &nodes, &devices, &event_tx);
                })
                .register();

            // Command listener
            let _command_listener = pipewire_rx.attach(mainloop.loop_(), move |command| {
                Self::handle_command(
                    &command,
                    &nodes_cmd.clone(),
                    &devices_cmd.clone(),
                    &metadata_cmd.clone(),
                    &mainloop_cmd.clone(),
                );
            });

            // New metadata listener
            let _meta_listener = meta_rx.attach(mainloop.loop_(), move |change| {
                Self::handle_metadata_change(&change, &event_tx_meta, &nodes_meta);
            });

            mainloop.run();

            Ok(())
        })
    }

    /// Handles incoming commands from pipewire
    fn handle_command(
        command: &Command,
        nodes: &RefCell<NodeMap>,
        devices: &RefCell<DeviceMap>,
        metadata: &RefCell<Option<MetadataTracker>>,
        mainloop: &MainLoop,
    ) {
        match command {
            Command::ChangeVolume(id, volume) => {
                if let Some(node) = nodes.borrow().get(id) {
                    // Set volume through the associated device if it exists.
                    if let Some((device, hints)) = associated_device(node, &devices.borrow()) {
                        device.set_route_volume(&hints, *volume);
                    } else {
                        node.set_volume(*volume);
                    }
                }
            }
            Command::Mute(id, muted) => {
                if let Some(node) = nodes.borrow().get(id) {
                    // Set mute status through the associated device if it exists
                    if let Some((device, hints)) = associated_device(node, &devices.borrow()) {
                        device.set_route_mute(&hints, *muted);
                    } else {
                        node.set_mute(*muted);
                    }
                }
            }
            Command::ChangeDefaultSource(id) => {
                let Some(node) = nodes.borrow().find_props(*id) else {
                    return;
                };

                let metadata = metadata.borrow();
                if let Some(ref metadata) = *metadata {
                    metadata.metadata.set_default_source(&node.name);
                }
            }
            Command::ChangeDefaultSink(id) => {
                let Some(node) = nodes.borrow().find_props(*id) else {
                    return;
                };

                let metadata = metadata.borrow();
                if let Some(ref metadata) = *metadata {
                    metadata.metadata.set_default_sink(&node.name);
                }
            }
            Command::Quit => {
                mainloop.quit();
            }
        }
    }

    /// Handle new incoming object from pipewire, usually this means creating the relevant
    /// tracker and adding it to its hashmap, and giving it an `event_tx` so it can send events
    /// back to us.
    fn handle_new_object(
        object: &GlobalObject<&DictRef>,
        nodes: &RefCell<NodeMap>,
        devices: &RefCell<DeviceMap>,
        metadata: &RefCell<Option<MetadataTracker>>,
        event_tx: &mpsc::Sender<Event>,
        metadata_tx: &pipewire::channel::Sender<MetadataChange>,
        registry: &Registry,
    ) {
        match object.type_ {
            ObjectType::Node => {
                let node = node::monitor_node(registry, object, &event_tx.clone());
                if let Some(node) = node {
                    nodes.borrow_mut().insert(ObjectId(object.id), node);
                }
            }
            ObjectType::Device => {
                let device = device::monitor_device(registry, object);
                if let Some(device) = device {
                    devices.borrow_mut().insert(ObjectId(object.id), device);
                }
            }
            ObjectType::Metadata => {
                // Only one metadata object exists. therefore we capture it only
                // initially, when `metadata` is `None`.
                if metadata.borrow().is_none() {
                    *metadata.borrow_mut() =
                        metadata::monitor_metadata(registry, object, metadata_tx);
                }
            }
            _ => {} // i dont care about anything esle xd
        }
    }

    /// Handles dropping removed pipewire objects from the hashmap
    fn handle_object_deletion(
        deleted_id: ObjectId,
        nodes: &RefCell<NodeMap>,
        devices: &RefCell<DeviceMap>,
        event_tx: &mpsc::Sender<Event>,
    ) {
        if nodes.borrow_mut().remove(&deleted_id).is_some() {
            let _ = event_tx.blocking_send(Event::NodeRemoved(deleted_id));
        }
        devices.borrow_mut().remove(&deleted_id);
    }

    /// Handles incoming changes to metadata, such as default device changes
    fn handle_metadata_change(
        change: &MetadataChange,
        event_tx: &mpsc::Sender<Event>,
        nodes: &RefCell<NodeMap>,
    ) {
        match change {
            // default speaker/headphones changed
            MetadataChange::DefaultSink(name) => {
                let Some(node) = nodes.borrow().find_props_by_name(name) else {
                    return;
                };

                let _ = event_tx.blocking_send(Event::DefaultSinkChanged(node.id));
            }
            // default microphone changed
            MetadataChange::DefaultSource(name) => {
                let Some(node) = nodes.borrow().find_props_by_name(name) else {
                    return;
                };

                let _ = event_tx.blocking_send(Event::DefaultSourceChanged(node.id));
            }
        }
    }
}

// Ergonomic methods for common operations on the node map
trait FindNode {
    fn find_props(&self, id: ObjectId) -> Option<Arc<NodeProps>>;

    fn find_props_by_name(&self, name: &str) -> Option<Arc<NodeProps>>;
}

impl FindNode for NodeMap {
    fn find_props(&self, id: ObjectId) -> Option<Arc<NodeProps>> {
        self.get(&id).map(|a| a.props.clone())
    }

    fn find_props_by_name(&self, name: &str) -> Option<Arc<NodeProps>> {
        self.iter()
            .find(|(_, node)| name.contains(&node.props.name))
            .map(|(_, node)| node.props.clone())
    }
}

/// Find the associated device for a node, if there is one.
fn associated_device<'a>(
    node: &NodeTracker,
    devices: &'a DeviceMap,
) -> Option<(&'a DeviceTracker, DeviceHints)> {
    let lock = node.props.device_hints.blocking_lock();
    let hints = lock.as_ref()?;
    let device = devices.get(&hints.device_id)?;

    Some((device, hints.clone()))
}
