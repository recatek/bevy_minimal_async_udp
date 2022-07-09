use bevy::prelude::*;
use bevy::tasks::{Task, TaskPool};

use async_net::{SocketAddr, UdpSocket};
use flume::{Receiver, Sender, TryRecvError, TrySendError};
use futures_lite::future;

const BUFFER_MAX_SIZE: usize = 2000;

/// An async-ready channel for communication between async tasks and bevy systems.
struct AsyncChannel<T> {
    pub sender: Sender<T>,
    pub receiver: Receiver<T>,
}

impl<T> AsyncChannel<T> {
    pub fn new() -> Self {
        let (sender, receiver) = flume::unbounded();
        Self { sender, receiver }
    }
}

/// A struct representing a simple address-tagged byte payload.
pub struct Message {
    /// The source or destination address.
    address: SocketAddr,
    // The raw bytes containing the message data.
    payload: Vec<u8>,
}

impl Message {
    pub fn new(address: SocketAddr, payload: Vec<u8>) -> Self {
        Self { address, payload }
    }

    pub fn address(&self) -> &SocketAddr {
        &self.address
    }

    pub fn payload(&self) -> &Vec<u8> {
        &self.payload
    }
}

/// Network resource struct, contains handles to two async task threads, one for sending,
/// and one for receiving, along with async message channels to communicate with each.
pub struct Network {
    send_task: Option<Task<()>>,
    recv_task: Option<Task<()>>,
    send_channel: AsyncChannel<Message>,
    recv_channel: AsyncChannel<Message>,
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

    pub fn startup(&mut self, port: u16, runtime: &TaskPool) {
        let socket = bind_socket(port);
        let send_relay = self.send_channel.receiver.clone();
        let recv_relay = self.recv_channel.sender.clone();
        self.send_task = Some(runtime.spawn(send_loop(socket.clone(), send_relay)));
        self.recv_task = Some(runtime.spawn(recv_loop(socket.clone(), recv_relay)));
    }

    pub fn parse_socket_addr(addr: &str) -> SocketAddr {
        addr.parse().unwrap()
    }
}

/// Binds a socket to the given port, using the local host address.
fn bind_socket(port: u16) -> UdpSocket {
    let bind_addr = SocketAddr::from(([127, 0, 0, 1], port));
    match future::block_on(UdpSocket::bind(bind_addr)) {
        Ok(socket) => socket,
        Err(err) => panic!("failed to listen on {}: {:?}", bind_addr, err),
    }
}

/// A network loop that awaits the next packet received by the UdpSocket
/// add adds it to the incoming message queue for consumption within bevy.
async fn send_loop(socket: UdpSocket, send_channel: Receiver<Message>) {
    loop {
        match send_channel.recv_async().await {
            Ok(message) => send_message(&socket, &message).await,
            Err(err) => warn!("failed to dequeue outgoing message: {:?}", err),
        };
    }
}

/// A network loop that awaits for a new message to be queued in the
/// outgoing message queue from bevy, and sends it via the given socket.
async fn recv_loop(socket: UdpSocket, recv_channel: Sender<Message>) {
    let mut buf = [0; BUFFER_MAX_SIZE];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((amt, src)) => recv_message(&src, &buf[..amt], &recv_channel).await,
            Err(err) => match err.kind() {
                std::io::ErrorKind::ConnectionReset => (), // Ignore ECONNRESET spam on recv
                _ => warn!("failed to recv packet: {:?}", err),
            },
        };
    }
}

/// Send a message out on a given UdpSocket. Uses the message address field as the target.
async fn send_message(socket: &UdpSocket, message: &Message) {
    if let Err(err) = socket.send_to(&message.payload, message.address).await {
        warn!("failed to send packet to {}: {:?}", message.address, err);
    }
}

/// Receives a message from the given address and adds it into the incoming message queue.
async fn recv_message(source: &SocketAddr, buf: &[u8], recv_channel: &Sender<Message>) {
    if let Err(err) = recv_channel.send(Message::new(*source, buf.to_vec())) {
        warn!("failed to enqueue incoming message: {:?}", err);
    }
}

/// Simple plugin for adding necessary network resources and functions.
pub struct NetworkPlugin {
    listen_port: u16,
}

impl NetworkPlugin {
    pub fn new(listen_port: u16) -> Self {
        NetworkPlugin { listen_port }
    }
}

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        // Add a startup system that incorporates the listen port we selected
        let port = self.listen_port;
        let startup = move |mut net: ResMut<Network>, runtime: Res<TaskPool>| {
            net.startup(port, runtime.into_inner())
        };

        app.init_resource::<TaskPool>()
            .insert_resource(Network::new())
            .add_startup_system(startup);
    }
}
