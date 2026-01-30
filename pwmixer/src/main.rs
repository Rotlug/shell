use std::{collections::HashMap, sync::mpsc};

use pwmixer::{Command, Event, mixer::Mixer};

fn main() {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (mixer, event_rx) = Mixer::new(cmd_rx);

    let mut nodes = HashMap::new();

    while let Ok(event) = event_rx.recv() {
        match event {
            Event::NodeAdded(node) => {
                nodes.insert(node.id, node);
            }
            Event::NodeRemoved(id) => {
                nodes.remove(&id);
            }
            Event::NodeVolumesChanged(id, vols) => {
                let node = nodes.get(&id).unwrap();
                println!("{node}: {vols:?}");
            }
            Event::NodeMuted(id, muted) => {
                let node = nodes.get(&id).unwrap();
                println!("{node} mute status changed: {muted}");
            }
            Event::DefaultSourceChanged(id) => {
                let node = nodes.get(&id).unwrap();
                println!("{node} is the default source now");
            }
            Event::DefaultSinkChanged(id) => {
                let node = nodes.get(&id).unwrap();
                println!("{node} is the default sink now");
            }
        }
    }

    let _ = cmd_tx.send(Command::Quit);
}
