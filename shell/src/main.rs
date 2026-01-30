use gtk::prelude::*;
use gtk4 as gtk;

use gtk::glib;

use gtk4_layer_shell::{Edge, Layer, LayerShell};

struct TestWindow;

const BAR_HEIGHT: i32 = 30;

impl TestWindow {
    fn activate(application: &gtk::Application) {
        let window = gtk::ApplicationWindow::new(application);

        window.init_layer_shell();

        /* Request top layer and exclusive real estate */
        window.set_layer(Layer::Top);
        window.set_height_request(BAR_HEIGHT);
        window.set_exclusive_zone(BAR_HEIGHT);

        /* Pin window to the top of the screen */
        let anchors = [
            (Edge::Left, true),
            (Edge::Right, true),
            (Edge::Bottom, false),
            (Edge::Top, true),
        ];

        for (anchor, state) in anchors {
            window.set_anchor(anchor, state);
        }

        /* Window Contents */

        // Left Section
        let left_box = gtk::Box::builder().hexpand(true).build();
        left_box.append(&gtk::Label::new(Some(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        )));

        // Center Section
        let center_box = gtk::Box::builder()
            .halign(gtk::Align::Center)
            // .hexpand(true)
            .build();
        center_box.append(&gtk::Label::new(Some("Center")));

        // Right Section
        let right_box = gtk::Box::builder()
            .hexpand(true)
            .halign(gtk::Align::End)
            .build();
        right_box.append(&gtk::Label::new(Some("Right")));

        // Root Overlay
        let parent_box = gtk::Overlay::builder().build();
        parent_box.set_child(Some(&left_box));
        parent_box.add_overlay(&center_box);
        parent_box.add_overlay(&right_box);

        window.set_child(Some(&parent_box));

        window.show();
    }
}

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("land.lugasi.shell")
        .build();

    app.connect_activate(TestWindow::activate);

    app.run()
}
