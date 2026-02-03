use brightness::monitor::get_all_monitors;

fn main() {
    let monitors = get_all_monitors();
    for mut monitor in monitors {
        monitor.set_brightness(50);
    }
}
