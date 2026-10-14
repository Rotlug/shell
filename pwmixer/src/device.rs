// Lots of this code is from https://github.com/tsowell/wiremix

use std::{collections::HashMap, sync::Arc};

use libspa::{
    param::ParamType,
    pod::{Object, Pod, Property, PropertyFlags, Value, ValueArray, serialize::PodSerializer},
    utils::dict::DictRef,
};

use pipewire::{
    device::Device,
    proxy::Listener,
    registry::{GlobalObject, Registry},
};
use tokio::sync::Mutex;

use crate::{ObjectId, deserialize::deserialize};

pub struct DeviceTracker {
    pub listener: Box<dyn Listener>,
    pub device: Device,
    pub id: ObjectId,

    /// Maps from card device to route index
    ///
    /// Key -> card.profile.device of `Node`
    /// Value -> Route index of `Device`
    route_map: Arc<Mutex<HashMap<i32, i32>>>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct DeviceHints {
    pub device_id: ObjectId,
    pub card_device: i32,
}

#[must_use]
pub fn monitor_device(
    registry: &Registry,
    object: &GlobalObject<&DictRef>,
) -> Option<DeviceTracker> {
    let id = ObjectId::from(object);

    let props = object.props?;
    let media_class = props.get("media.class")?;
    if media_class != "Audio/Device" {
        return None;
    }

    let device: Device = registry.bind(object).ok()?;

    let params = [
        ParamType::EnumRoute,
        ParamType::Route,
        ParamType::Profile,
        ParamType::EnumProfile,
    ];

    let route_map = Arc::new(Mutex::new(HashMap::new()));
    let route_map_param = route_map.clone();

    let listener = device
        .add_listener_local()
        .param(move |_seq, id, _index, _next, params| {
            let Some(obj) = deserialize(params) else {
                return;
            };

            if id == ParamType::Route {
                device_new_params(&obj, &route_map_param);
            }
        })
        .register();

    device.subscribe_params(&params);

    Some(DeviceTracker {
        listener: Box::new(listener),
        device,
        id,
        route_map,
    })
}

/// Handles parameter changes for device
fn device_new_params(object: &Object, route_map: &Mutex<HashMap<i32, i32>>) {
    let mut route_index = None;
    let mut card_device = None;

    for property in &object.properties {
        match property.key {
            libspa_sys::SPA_PARAM_ROUTE_index => {
                if let Value::Int(index) = property.value {
                    route_index = Some(index);
                }
            }
            libspa_sys::SPA_PARAM_ROUTE_device => {
                if let Value::Int(device) = property.value {
                    card_device = Some(device);
                }
            }
            _ => {}
        }
    }

    if let (Some(idx), Some(dev)) = (route_index, card_device) {
        route_map.blocking_lock().insert(dev, idx);
    }
}

impl DeviceTracker {
    /// Set the properties for a specific route on this device.
    ///
    /// If the `device_id` in `hints` does not much the id of this current device, or this
    /// device has no route for `hints.card_device` (e.g. bluez5 devices, which don't expose
    /// per-route volume control the way ALSA cards do), nothing happens and `false` is
    /// returned so the caller can fall back to setting the node's own volume/mute directly.
    fn set_route_properties(&self, hints: &DeviceHints, properties: Vec<Property>) -> bool {
        if self.id != hints.device_id {
            return false;
        }

        // Find the route index corresponding to the `card_device` in the map
        let Some(route) = self
            .route_map
            .blocking_lock()
            .get(&hints.card_device)
            .copied()
        else {
            return false;
        };

        let mut route_properties = vec![
            Property {
                key: libspa_sys::SPA_PARAM_ROUTE_index,
                flags: PropertyFlags::empty(),
                value: Value::Int(route),
            },
            Property {
                key: libspa_sys::SPA_PARAM_ROUTE_device,
                flags: PropertyFlags::empty(),
                value: Value::Int(hints.card_device),
            },
        ];

        if !properties.is_empty() {
            route_properties.push(Property {
                key: libspa_sys::SPA_PARAM_ROUTE_props,
                flags: PropertyFlags::empty(),
                value: Value::Object(Object {
                    type_: libspa_sys::SPA_TYPE_OBJECT_Props,
                    id: libspa_sys::SPA_PARAM_Route,
                    properties,
                }),
            });
        }

        route_properties.push(Property {
            key: libspa_sys::SPA_PARAM_ROUTE_save,
            flags: PropertyFlags::empty(),
            value: Value::Bool(true),
        });

        let values = PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &Value::Object(Object {
                type_: libspa_sys::SPA_TYPE_OBJECT_ParamRoute,
                id: libspa_sys::SPA_PARAM_Route,
                properties: route_properties,
            }),
        );

        if let Ok((values, _)) = values
            && let Some(pod) = Pod::from_bytes(&values.into_inner())
        {
            self.device.set_param(ParamType::Route, 0, pod);
            return true;
        }

        false
    }

    /// Set the volume for a specific route on this device. Returns `false` if this device has
    /// no matching route, in which case the caller should fall back to `Node::set_volume`.
    pub(crate) fn set_route_volume(&self, hints: &DeviceHints, volume: f32) -> bool {
        let volume = volume.powi(3).clamp(0.0, 1.0);

        self.set_route_properties(
            hints,
            vec![Property {
                key: libspa_sys::SPA_PROP_channelVolumes,
                flags: PropertyFlags::empty(),
                value: Value::ValueArray(ValueArray::Float(vec![volume])),
            }],
        )
    }

    /// Set wether the device is muted or not. Returns `false` if this device has no matching
    /// route, in which case the caller should fall back to `Node::set_mute`.
    pub(crate) fn set_route_mute(&self, hints: &DeviceHints, muted: bool) -> bool {
        self.set_route_properties(
            hints,
            vec![Property {
                key: libspa_sys::SPA_PROP_mute,
                flags: PropertyFlags::empty(),
                value: Value::Bool(muted),
            }],
        )
    }
}
