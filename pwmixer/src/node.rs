// Lots of this code is from https://github.com/tsowell/wiremix

use std::{
    fmt::Display,
    rc::Rc,
    sync::{Arc, mpsc::Sender},
};

use libspa::{
    param::ParamType,
    pod::{Object, Pod, Property, PropertyFlags, Value, ValueArray, serialize::PodSerializer},
    utils::dict::DictRef,
};
use pipewire::{
    node::Node,
    proxy::Listener,
    registry::{GlobalObject, Registry},
};

use crate::{Event, ObjectId, deserialize::deserialize};

#[derive(PartialEq, Eq, Hash, Debug)]
pub enum NodeKind {
    Source,
    Sink,
    AppOut,
    AppIn,
}

/// Represents a Mic, Speaker, Headphone, or an application that is playing some audio.
#[derive(PartialEq, Eq, Hash, Debug)]
pub struct NodeProps {
    pub name: String,
    pub description: Option<String>,
    pub nick: Option<String>,
    pub app_name: Option<String>,
    pub id: ObjectId,
    pub kind: NodeKind,
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
    event_tx: &Sender<Event>,
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
    });

    let node_info_clone = node_props.clone();
    let event_tx_clone = event_tx.clone();
    let listener = node
        .add_listener_local()
        .param(move |_, _, _, _, params| {
            let Some(obj) = deserialize(params) else {
                return;
            };

            node_new_params(obj, &node_info_clone, &event_tx_clone.clone());
        })
        .register();

    node.subscribe_params(&[ParamType::Props]);

    let _ = event_tx.send(Event::NodeAdded(node_props.clone()));

    // also return the listener because if i don't then it gets dropped and stops working
    Some(NodeTracker {
        node: Rc::new(node),
        props: node_props,
        listener: Box::new(listener),
    })
}

/// Send events to `event_tx` based on which new params have been sent through pipewire.
pub fn node_new_params(obj: Object, node: &Arc<NodeProps>, event_tx: &Sender<Event>) {
    for prop in obj.properties {
        match prop.key {
            libspa_sys::SPA_PROP_channelVolumes => {
                if let Value::ValueArray(libspa::pod::ValueArray::Float(values)) = prop.value {
                    let volumes = values
                        .iter()
                        .map(|vol| {
                            /* NOTE: I don't know why but doing this makes
                             * the resulting number the same as the value i see in the kde
                             * plasma volume meter so im guessing its more accure to
                             * how humans hear the sound or something
                             */
                            vol.powf(1.0 / 3.0)
                        })
                        .collect();

                    let _ = event_tx.send(Event::NodeVolumesChanged(node.id, volumes));
                }
            }
            libspa_sys::SPA_PROP_mute => {
                if let Value::Bool(value) = prop.value {
                    let _ = event_tx.send(Event::NodeMuted(node.id, value));
                }
            }
            _ => (),
        }
    }
}

pub trait NodeControl {
    /// Set any properties of this node,
    /// NOTE: Properties that are not specified in the `properties` Vec will be
    /// unchanged.
    fn set_properties(&self, properties: Vec<Property>);

    /// Set the value of each channels volume in the node.
    /// A `Vec` is needed here because most nodes are stereo, so they have more than
    /// one channel (L/R).
    ///
    /// Most of the time you don't care about setting the volumes of each channels
    /// individually, in which case its more ergonomic to call `set_volume`
    fn set_volumes(&self, volumes: Vec<f32>);

    /// A wrapper around `set_volumes` that wraps the given `volume` in a `Vec`.
    /// This sets each channel volume in the node to the same `volume`.
    fn set_volume(&self, volume: f32);

    fn set_mute(&self, muted: bool);
}

impl NodeControl for Node {
    fn set_properties(&self, properties: Vec<Property>) {
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
            self.set_param(ParamType::Props, 0, pod);
        }
    }

    fn set_volumes(&self, volumes: Vec<f32>) {
        let volumes = volumes.iter().map(|f| f.powi(3)).collect();

        self.set_properties(vec![Property {
            key: libspa_sys::SPA_PROP_channelVolumes,
            flags: PropertyFlags::empty(),
            value: Value::ValueArray(ValueArray::Float(volumes)),
        }]);
    }

    fn set_volume(&self, volume: f32) {
        self.set_volumes(vec![volume]);
    }

    fn set_mute(&self, muted: bool) {
        self.set_properties(vec![
            Property {
                key: libspa_sys::SPA_PROP_mute,
                flags: PropertyFlags::empty(),
                value: Value::Bool(muted),
            },
            Property {
                key: libspa_sys::SPA_PROP_mute,
                flags: PropertyFlags::empty(),
                value: Value::Bool(muted),
            },
        ]);
    }
}
