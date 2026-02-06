use std::collections::HashMap;

use pwmixer::{Command, Event, Mixer};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let (mixer, mut event_rx) = Mixer::new(cmd_rx);

    let mut nodes = HashMap::new();

    while let Some(event) = event_rx.recv().await {
        match event {
            Event::NodeAdded(node) => {
                nodes.insert(node.id, node);
            }
            Event::NodeRemoved(id) => {
                nodes.remove(&id);
            }
            Event::NodeVolumeChanged(id, vol) => {
                let node = nodes.get(&id).unwrap();
                println!("{node}: {vol:?}");
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

    let _ = cmd_tx.send(Command::Quit).await;
}
