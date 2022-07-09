use bevy::log::LogPlugin;
use bevy::prelude::*;

use common::{Message, Network, NetworkPlugin};

fn main() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugin(LogPlugin::default());
    app.add_plugin(NetworkPlugin::new(0)); // Pick no listen port since we're a client
    app.add_system(print_network_messages);
    app.add_system(send_client_message);
    app.run();
}

fn print_network_messages(net: Res<Network>) {
    while let Ok(message) = net.try_recv() {
        match std::str::from_utf8(&message.payload()) {
            Ok(text) => info!("got: \"{}\" from: {}", text, &message.address()),
            Err(_) => warn!("got malformed string from: {}", &message.address()),
        };
    }
}

fn send_client_message(net: Res<Network>) {
    let target_addr = Network::parse_socket_addr("127.0.0.1:34243");
    let reply = Message::new(target_addr, "message!".as_bytes().to_vec());
    if net.try_send(reply).is_err() {
        warn!("failed to send message");
    }
}
