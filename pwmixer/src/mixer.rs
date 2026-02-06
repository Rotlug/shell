use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    thread::{self, JoinHandle},
};

use pipewire::{context::ContextRc, main_loop::MainLoopRc, node::Node, types::ObjectType};
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
        let handle = thread::spawn(move || -> crate::Result<()> {
            let mainloop = MainLoopRc::new(None)?;
            let context = ContextRc::new(&mainloop, None)?;
            let core = context.connect_rc(None)?;
            let registry = core.get_registry_rc()?;

            let registry_clone = registry.clone();

            let nodes: Rc<RefCell<NodeMap>> = Rc::new(RefCell::new(HashMap::new()));
            let devices: Rc<RefCell<DeviceMap>> = Rc::new(RefCell::new(HashMap::new()));

            let metadata: Rc<RefCell<Option<MetadataTracker>>> = Rc::new(RefCell::new(None));

            let nodes_event = nodes.clone();
            let nodes_cmd = nodes.clone();
            let node_meta = nodes.clone();

            let devices_event = devices.clone();
            let devices_command = devices.clone();

            let metadata_event = metadata.clone();
            let metadata_cmd = metadata;

            let (meta_tx, meta_rx) = pipewire::channel::channel();

            let event_tx_event = event_tx.clone();
            let event_tx_meta = event_tx.clone();

            let mainloop_cmd = mainloop.clone();

            // Event listener
            let _event_listener = registry
                .add_listener_local()
                .global(move |object| {
                    let node = if object.type_ == ObjectType::Node {
                        node::monitor_node(&registry_clone, object, &event_tx.clone())
                    } else {
                        None
                    };

                    let device = if object.type_ == ObjectType::Device {
                        device::monitor_device(&registry_clone, object)
                    } else {
                        None
                    };

                    if metadata_event.borrow().is_none() {
                        *metadata_event.borrow_mut() = if object.type_ == ObjectType::Metadata {
                            metadata::monitor_metadata(&registry_clone, object, &meta_tx)
                        } else {
                            None
                        }
                    }

                    if let Some(node) = node {
                        nodes_event.borrow_mut().insert(ObjectId(object.id), node);
                    }

                    if let Some(device) = device {
                        devices_event
                            .borrow_mut()
                            .insert(ObjectId(object.id), device);
                    }
                })
                .global_remove(move |id| {
                    let removed = nodes.borrow_mut().remove(&ObjectId(id));
                    if removed.is_some() {
                        let _ = event_tx_event.blocking_send(Event::NodeRemoved(ObjectId(id)));
                    }
                })
                .register();

            // Command listener
            let _command_listener =
                pipewire_rx.attach(mainloop.loop_(), move |command| match command {
                    Command::ChangeVolume(id, volume) => {
                        if let Some(node) = nodes_cmd.borrow().get(&id) {
                            // Set volume through the associated device if it exists.
                            if let Some((device, hints)) =
                                associated_device(node, &devices_command.borrow())
                            {
                                device.set_route_volume(&hints, volume);
                            } else {
                                node.set_volume(volume);
                            }
                        }
                    }
                    Command::Mute(id, muted) => {
                        if let Some(node) = nodes_cmd.borrow().get(&id) {
                            // Set mute status through the associated device if it exists
                            if let Some((device, hints)) =
                                associated_device(node, &devices_command.borrow())
                            {
                                device.set_route_mute(&hints, muted);
                            } else {
                                node.set_mute(muted);
                            }
                        }
                    }
                    Command::ChangeDefaultSource(id) => {
                        let Some(node) = nodes_cmd.borrow().find_props(id) else {
                            return;
                        };

                        let metadata = metadata_cmd.borrow();
                        if let Some(ref metadata) = *metadata {
                            metadata.metadata.set_default_source(&node.name);
                        }
                    }
                    Command::ChangeDefaultSink(id) => {
                        let Some(node) = nodes_cmd.borrow().find_props(id) else {
                            return;
                        };

                        let metadata = metadata_cmd.borrow();
                        if let Some(ref metadata) = *metadata {
                            metadata.metadata.set_default_sink(&node.name);
                        }
                    }
                    Command::Quit => {
                        mainloop_cmd.quit();
                    }
                });

            let _meta_listener =
                meta_rx.attach(mainloop.loop_(), move |metachange| match metachange {
                    MetadataChange::DefaultSink(name) => {
                        let Some(node) = node_meta.borrow().find_props_by_name(name) else {
                            return;
                        };

                        let _ = event_tx_meta.blocking_send(Event::DefaultSinkChanged(node.id));
                    }
                    MetadataChange::DefaultSource(name) => {
                        let Some(node) = node_meta.borrow().find_props_by_name(name) else {
                            return;
                        };

                        let _ = event_tx_meta.blocking_send(Event::DefaultSourceChanged(node.id));
                    }
                });

            mainloop.run();

            Ok(())
        });

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
}

// Ergonomic methods for common operations on the node map
trait FindNode {
    fn find_props(&self, id: ObjectId) -> Option<Arc<NodeProps>>;

    fn find_props_by_name(&self, name: String) -> Option<Arc<NodeProps>>;
}

impl FindNode for NodeMap {
    fn find_props(&self, id: ObjectId) -> Option<Arc<NodeProps>> {
        self.get(&id).map(|a| a.props.clone())
    }

    fn find_props_by_name(&self, name: String) -> Option<Arc<NodeProps>> {
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
