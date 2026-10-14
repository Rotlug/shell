use std::rc::Rc;

use pipewire::{
    channel::Sender,
    metadata::Metadata,
    proxy::Listener,
    registry::{GlobalObject, Registry},
};

use libspa::utils::dict::DictRef;
use serde_json::json;

use crate::{MetadataChange, ObjectId};

pub struct MetadataTracker {
    pub metadata: Rc<Metadata>,
    pub _listener: Box<dyn Listener>,
    pub _id: ObjectId,
}

// `default.configured.audio.*` is the user's override *preference* - it's
// only ever set after something explicitly changes the default, so it's
// absent at startup even though a default sink/source is already active,
// and it never changes on its own when WirePlumber auto-switches the
// resolved default (e.g. a Bluetooth device connecting and becoming the
// new default sink). `default.audio.*` is the actually-resolved current
// default (always populated, updated on every resolution change), so
// listening must watch that key instead; writes still target the
// "configured" key, since that's the one WirePlumber treats as the
// override to resolve against.
const DEFAULT_SINK: &str = "default.audio.sink";
const DEFAULT_SOURCE: &str = "default.audio.source";
const DEFAULT_CONFIGURED_SINK: &str = "default.configured.audio.sink";
const DEFAULT_CONFIGURED_SOURCE: &str = "default.configured.audio.source";

#[must_use]
pub fn monitor_metadata(
    registry: &Registry,
    object: &GlobalObject<&DictRef>,
    meta_tx: &Sender<MetadataChange>,
) -> Option<MetadataTracker> {
    let object_id = ObjectId::from(object);

    let props = object.props?;
    let metadata_name = props.get("metadata.name")?;
    if metadata_name != "default" {
        return None;
    }

    let metadata: Metadata = registry.bind(object).ok()?;
    let metadata = Rc::new(metadata);

    let meta_tx = meta_tx.clone();

    let listener = metadata
        .add_listener_local()
        .property({
            move |_, key, _, value| {
                let Some(value) = value else {
                    return 0;
                };

                match key {
                    Some(DEFAULT_SINK) => {
                        let _ = meta_tx.send(MetadataChange::DefaultSink(value.to_string()));
                    }
                    Some(DEFAULT_SOURCE) => {
                        let _ = meta_tx.send(MetadataChange::DefaultSource(value.to_string()));
                    }
                    _ => {}
                }

                0
            }
        })
        .register();

    Some(MetadataTracker {
        _id: object_id,
        _listener: Box::new(listener),
        metadata,
    })
}

pub trait MetadataControl {
    fn set_prop_json(&self, key: &str, value: Option<&str>);

    fn set_default_source(&self, name: &str);

    fn set_default_sink(&self, name: &str);
}

impl MetadataControl for Metadata {
    fn set_prop_json(&self, key: &str, value: Option<&str>) {
        self.set_property(0, key, Some("Spa:String:JSON"), value);
    }

    fn set_default_source(&self, name: &str) {
        self.set_prop_json(DEFAULT_CONFIGURED_SOURCE, Some(name));
    }

    fn set_default_sink(&self, name: &str) {
        let name = &json!({"name": name}).to_string();
        self.set_prop_json(DEFAULT_CONFIGURED_SINK, Some(name));
    }
}
