// From -> https://github.com/tsowell/wiremix
//! Type for representing `PipeWire` object IDs.
use std::fmt::Display;

use libspa::utils::dict::DictRef;
use pipewire::registry::GlobalObject;

/// A `PipeWire` object ID.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ObjectId(pub u32);

impl From<&GlobalObject<&DictRef>> for ObjectId {
    fn from(obj: &GlobalObject<&DictRef>) -> Self {
        Self(obj.id)
    }
}

impl From<ObjectId> for u32 {
    fn from(id: ObjectId) -> Self {
        id.0
    }
}

impl Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.to_string().as_str())
    }
}

impl ObjectId {
    #[must_use]
    pub const fn from_raw_id(id: u32) -> Self {
        Self(id)
    }
}
