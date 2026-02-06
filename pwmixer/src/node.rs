// Lots of this code is from https://github.com/tsowell/wiremix

use std::{fmt::Display, rc::Rc, sync::Arc};

use libspa::{
    param::ParamType,
    pod::{Object, Pod, Property, PropertyFlags, Value, serialize::PodSerializer},
    utils::dict::DictRef,
};

use pipewire::{
    node::{Node, NodeChangeMask, NodeInfoRef},
    proxy::Listener,
    registry::{GlobalObject, Registry},
};

use tokio::sync::{Mutex, mpsc};

use crate::{Event, ObjectId, deserialize::deserialize, device::DeviceHints};

#[derive(PartialEq, Eq, Hash, Debug)]
pub enum NodeKind {
    Source,
    Sink,
    AppOut,
    AppIn,
}

/// Represents a Mic, Speaker, Headphone, or an application that is playing some audio.
#[derive(Debug)]
pub struct NodeProps {
    pub name: String,
    pub description: Option<String>,
    pub nick: Option<String>,
    pub app_name: Option<String>,
    pub id: ObjectId,
    pub kind: NodeKind,

    pub(crate) device_hints: Mutex<Option<DeviceHints>>,
}

impl Display for NodeProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(nick) = &self.nick {
            f.write_str(nick)
        } else if let Some(app) = &self.app_name {
            f.write_str(app)
        } else if let Some(desc) = &self.description {
            f.write_str(desc)
        } else {
            f.write_str(&self.name)
        }
    }
}

pub struct NodeTracker {
    pub listener: Box<dyn Listener>,
    pub props: Arc<NodeProps>,
    pub node: Rc<Node>,
}

/// Checks if an object is an audio node and then creates a listeners that emits
/// events when the node's props changes. If the node is not an audio node, returns `None`.
#[must_use]
pub fn monitor_node(
    registry: &Registry,
    object: &GlobalObject<&DictRef>,
    event_tx: &mpsc::Sender<Event>,
) -> Option<NodeTracker> {
    let props = object.props?;

    let media_class = props.get("media.class")?;

    let id: ObjectId = object.into();

    let kind = match media_class {
        "Audio/Sink" => Some(NodeKind::Sink),
        "Audio/Source" => Some(NodeKind::Source),
        "Stream/Output/Audio" => Some(NodeKind::AppOut),
        "Stream/Input/Audio" => Some(NodeKind::AppIn),
        _ => return None,
    }?;

    let node: Node = registry.bind(object).ok()?;

    /* Get the various node names */
    let nick = props
        .get("node.nick")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let name = props
        .get("node.name")
        .filter(|s| !s.is_empty())?
        .to_string();
    let description = props
        .get("node.description")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let app_name = props
        .get("application.name")
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // The node itself
    let node_props = Arc::new(NodeProps {
        name,
        description,
        nick,
        app_name,
        id,
        kind,
        device_hints: Mutex::new(None),
    });

    let node_props_params = node_props.clone();
    let node_props_info = node_props.clone();
    let event_tx_clone = event_tx.clone();
    let listener = node
        .add_listener_local()
        .param(move |_, _, _, _, params| {
            let Some(obj) = deserialize(params) else {
                return;
            };

            node_new_params(obj, &node_props_params, &event_tx_clone.clone());
        })
        .info(move |info| {
            for change in info.change_mask().iter() {
                if change == NodeChangeMask::PROPS {
                    node_new_info(info, &node_props_info);
                }
            }
        })
        .register();

    node.subscribe_params(&[ParamType::Props]);

    let _ = event_tx.blocking_send(Event::NodeAdded(node_props.clone()));

    // also return the listener because if i don't then it gets dropped and stops working
    Some(NodeTracker {
        node: Rc::new(node),
        props: node_props,
        listener: Box::new(listener),
    })
}

pub fn node_new_info(info: &NodeInfoRef, node: &NodeProps) -> Option<()> {
    let props = info.props()?;

    let device_id = props.get("device.id")?.parse::<u32>().ok()?;
    let card_device = props.get("card.profile.device")?.parse::<i32>().ok()?;

    *node.device_hints.blocking_lock() = Some(DeviceHints {
        device_id: ObjectId(device_id),
        card_device,
    });

    None
}

/// Send events to `event_tx` based on which new params have been sent through pipewire.
pub fn node_new_params(obj: Object, node: &NodeProps, event_tx: &mpsc::Sender<Event>) {
    for prop in obj.properties {
        match prop.key {
            libspa_sys::SPA_PROP_channelVolumes => {
                if let Value::ValueArray(libspa::pod::ValueArray::Float(vols)) = prop.value
                    && let Some(vol) = vols.first()
                {
                    let value = vol.cbrt();
                    let _ = event_tx.blocking_send(Event::NodeVolumeChanged(node.id, value));
                }
            }
            libspa_sys::SPA_PROP_mute => {
                if let Value::Bool(value) = prop.value {
                    let _ = event_tx.blocking_send(Event::NodeMuted(node.id, value));
                }
            }
            _ => (),
        }
    }
}

impl NodeTracker {
    /// Set any properties of this node,
    /// NOTE: Properties that are not specified in the `properties` Vec will be
    /// unchanged.
    pub fn set_properties(&self, properties: Vec<Property>) {
        let values = PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &Value::Object(Object {
                type_: libspa_sys::SPA_TYPE_OBJECT_Props,
                id: libspa_sys::SPA_PARAM_Props,
                properties,
            }),
        );

        if let Ok((values, _)) = values
            && let Some(pod) = Pod::from_bytes(&values.into_inner())
        {
            self.node.set_param(ParamType::Props, 0, pod);
        }
    }

    /// Sets the nodes volume.
    /// NOTE: If the node is associated with a device, its better to find the device using
    /// `props.device_hints` and change the volume using the device's `set_route_volume`
    /// parameter.
    pub fn set_volume(&self, volume: f32) {
        let volume = volume.powi(3).clamp(0.0, 1.0);

        self.set_properties(vec![Property {
            key: libspa_sys::SPA_PROP_channelVolumes,
            flags: PropertyFlags::empty(),
            value: Value::ValueArray(libspa::pod::ValueArray::Float(vec![volume])),
        }]);
    }

    pub fn set_mute(&self, muted: bool) {
        self.set_properties(vec![Property {
            key: libspa_sys::SPA_PROP_mute,
            flags: PropertyFlags::empty(),
            value: Value::Bool(muted),
        }]);
    }
}
