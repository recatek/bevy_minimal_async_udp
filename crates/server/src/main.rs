use std::ops::Deref;
use std::str;

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::tasks::{Task, TaskPool, TaskPoolBuilder};

use async_net::{SocketAddr, UdpSocket};
use flume::{Receiver, Sender, TryRecvError, TrySendError};
use futures_lite::future;

const BUFFER_MAX_SIZE: usize = 2000;

pub struct AsyncChannel<T> {
    pub sender: Sender<T>,
    pub receiver: Receiver<T>,
}

impl<T> AsyncChannel<T> {
    pub fn new() -> Self {
        let (sender, receiver) = flume::unbounded();
        Self { sender, receiver }
    }
}

pub struct Message {
    address: SocketAddr, // The source or destination address
    payload: Vec<u8>,    // The raw bytes containing the message data
}

impl Message {
    pub fn new(address: SocketAddr, payload: Vec<u8>) -> Self {
        Self { address, payload }
    }
}

pub struct Network {
    send_task: Option<Task<()>>,
    recv_task: Option<Task<()>>,
    send_channel: AsyncChannel<Message>,
    recv_channel: AsyncChannel<Message>,
}

fn bind_socket(socket_addr: SocketAddr) -> UdpSocket {
    match future::block_on(UdpSocket::bind(socket_addr)) {
        Ok(socket) => socket,
        Err(err) => panic!("failed to listen on {}: {:?}", socket_addr, err),
    }
}

async fn send_loop(socket: UdpSocket, send_channel: Receiver<Message>) {
    loop {
        match send_channel.recv() {
            Ok(message) => send_message(&socket, &message).await,
            Err(err) => warn!("failed to dequeue outgoing message: {:?}", err),
        };
    }
}

async fn send_message(socket: &UdpSocket, message: &Message) {
    if let Err(err) = socket.send_to(&message.payload, message.address).await {
        warn!("failed to send packet to {}: {:?}", message.address, err);
    }
}

async fn recv_loop(socket: UdpSocket, recv_channel: Sender<Message>) {
    let mut buf = [0; BUFFER_MAX_SIZE];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((amt, src)) => recv_message(&src, &buf[..amt], &recv_channel).await,
            Err(err) => warn!("failed to recv packet: {:?}", err),
        };
    }
}

async fn recv_message(source: &SocketAddr, buf: &[u8], recv_channel: &Sender<Message>) {
    if let Err(err) = recv_channel.send(Message::new(*source, buf.to_vec())) {
        warn!("failed to enqueue incoming message: {:?}", err);
    }
}

impl Network {
    pub fn new() -> Self {
        Self {
            send_task: None,
            recv_task: None,
            send_channel: AsyncChannel::new(),
            recv_channel: AsyncChannel::new(),
        }
    }

    pub fn try_recv(&self) -> Result<Message, TryRecvError> {
        self.recv_channel.receiver.try_recv()
    }

    pub fn try_send(&self, message: Message) -> Result<(), TrySendError<Message>> {
        self.send_channel.sender.try_send(message)
    }

    pub fn startup(&mut self, socket_addr: SocketAddr, runtime: &TaskPool) {
        let socket = bind_socket(socket_addr);
		info!("listening on {}", socket_addr);
        let send_relay = self.send_channel.receiver.clone();
        let recv_relay = self.recv_channel.sender.clone();
        self.send_task = Some(runtime.spawn(send_loop(socket.clone(), send_relay)));
        self.recv_task = Some(runtime.spawn(recv_loop(socket.clone(), recv_relay)));
    }
}

fn main() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugin(LogPlugin::default());
    app.insert_resource(TaskPoolBuilder::new().build());
    app.insert_resource(Network::new());
    app.add_startup_system(start_server);
    app.add_system(print_network_messages);
    app.run();
}

// Bevy system-level code here -----------------------------------

fn start_server(mut net: ResMut<Network>, runtime: Res<TaskPool>) {
    net.startup("127.0.0.1:34243".parse().unwrap(), runtime.deref());
}

fn print_network_messages(net: Res<Network>) {
    while let Ok(message) = net.try_recv() {
        match str::from_utf8(&message.payload) {
            Ok(text) => {
                info!("got: \"{}\" from: {}", text, &message.address);
                let reply = Message::new(message.address, "received!".as_bytes().to_vec());
                if let Err(_) = net.try_send(reply) {
                    warn!("failed to send reply");
                }
            }
            Err(_) => warn!("got malformed string from: {}", &message.address),
        };
    }
}
