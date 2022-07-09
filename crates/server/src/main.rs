use bevy::log::LogPlugin;
use bevy::prelude::*;

use common::{Message, Network, NetworkPlugin};

fn main() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugin(LogPlugin::default());
    app.add_plugin(NetworkPlugin::new(34243)); // Set the listen port to 34243
    app.add_system(print_network_messages);
    app.run();
}

fn print_network_messages(net: Res<Network>) {
    while let Ok(message) = net.try_recv() {
        match std::str::from_utf8(message.payload()) {
            Ok(text) => info!("got: \"{}\" from: {}", text, &message.address()),
            Err(_) => warn!("got malformed string from: {}", &message.address()),
        };

        let reply = Message::new(*message.address(), "reply!".as_bytes().to_vec());
        if net.try_send(reply).is_err() {
            warn!("failed to send reply");
        }
    }
}
