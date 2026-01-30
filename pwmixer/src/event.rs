use std::sync::Arc;

use crate::{ObjectId, node::NodeProps};

#[derive(Debug)]
pub enum Event {
    NodeAdded(Arc<NodeProps>),
    NodeRemoved(ObjectId),
    NodeVolumesChanged(ObjectId, Vec<f32>),
    NodeMuted(ObjectId, bool),
    DefaultSourceChanged(ObjectId),
    DefaultSinkChanged(ObjectId),
}

#[derive(Debug)]
pub enum Command {
    ChangeVolumes(ObjectId, Vec<f32>),
    ChangeVolume(ObjectId, f32),
    Mute(ObjectId, bool),
    ChangeDefaultSource(ObjectId),
    ChangeDefaultSink(ObjectId),
    Quit,
}

pub enum MetadataChange {
    DefaultSink(String),
    DefaultSource(String),
}
