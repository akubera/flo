use server::engine::api::{ConnectionId, ServerMessage, ClientConnect};
use flo_event::{FloEvent, OwnedFloEvent, FloEventId};

use futures::sync::mpsc::UnboundedSender;

use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

static SEND_ERROR_DESC: &'static str = "Failed to send message through Client Channel";

#[derive(Debug, PartialEq)]
pub struct ClientSendError(ServerMessage);

impl ClientSendError {
    fn into_message(self) -> ServerMessage {
        self.0
    }
}

impl ::std::error::Error for ClientSendError {
    fn description(&self) -> &str {
        SEND_ERROR_DESC
    }
}

impl ::std::fmt::Display for ClientSendError {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(formatter, "{}", SEND_ERROR_DESC)
    }
}

pub enum ConsumerState {
    NotConsuming(FloEventId),
    ConsumeForward(FloEventId),
}

pub struct Client {
    connection_id: ConnectionId,
    addr: SocketAddr,
    sender: UnboundedSender<ServerMessage>,
    consumer_state: ConsumerState,
}

impl Client {
    pub fn from_client_connect(connect_message: ClientConnect) -> Client {
        Client {
            connection_id: connect_message.connection_id,
            addr: connect_message.client_addr,
            sender: connect_message.message_sender,
            consumer_state: ConsumerState::NotConsuming(FloEventId::new(0, 0)),
        }
    }

    pub fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn send(&mut self, message: ServerMessage) -> Result<(), ClientSendError> {
        trace!("Sending message to client: {} : {:?}", self.connection_id, message);
        self.sender.send(message).map_err(|send_err| {
            ClientSendError(send_err.into_inner())
        })
    }

    pub fn update_marker(&mut self, new_marker: FloEventId) {
        trace!("Client {} updating marker to: {:?}", self.connection_id, new_marker);
        let new_state = match self.consumer_state {
            ConsumerState::NotConsuming(_) => ConsumerState::NotConsuming(new_marker),
            ConsumerState::ConsumeForward(_) => ConsumerState::ConsumeForward(new_marker),
        };
        self.consumer_state = new_state;
    }
}

pub trait ClientManager {
    fn add_connection(&mut self, client_connect: ClientConnect);
    fn send_event(&mut self, event_producer: ConnectionId, event: OwnedFloEvent);
    fn send_message(&mut self, recipient: ConnectionId, message: ServerMessage) -> Result<(), ClientSendError>;
    fn update_marker(&mut self, connection: ConnectionId, marker: FloEventId);
}

pub struct ClientManagerImpl {
    client_map: HashMap<ConnectionId, Client>
}

impl ClientManagerImpl {
    pub fn new() -> ClientManagerImpl {
        ClientManagerImpl {
            client_map: HashMap::with_capacity(128),
        }
    }
}

impl ClientManager for ClientManagerImpl {

    fn add_connection(&mut self, client_connect: ClientConnect) {
        let connection_id = client_connect.connection_id;
        let client_count = self.client_map.len() + 1;
        debug!("Adding Client with connection_id: {}, peer_addr: {} total_connections_open: {}",
               connection_id,
               &client_connect.client_addr,
               client_count);
        let client = Client::from_client_connect(client_connect);
        self.client_map.insert(connection_id, client);
    }

    fn send_event(&mut self, event_producer: ConnectionId, event: OwnedFloEvent) {
        let event_id = event.id;
        let event_arc = Arc::new(event);
        let mut clients_to_remove = Vec::new();
        for mut client in self.client_map.values_mut() {
            let client_id = client.connection_id();
            if client_id != event_producer {
                debug!("Sending event: {:?} to client: {}", event_id, client_id);
                if let Err(err) = client.send(ServerMessage::Event(event_arc.clone())) {
                    warn!("Failed to send event: {:?} through client channel. Client likely just disconnected. ConnectionId: {}",
                          event_id,
                          client_id);
                    clients_to_remove.push(client_id);
                } else {
                    debug!("sent event: {:?} to client channel: {}", event_id, client_id);
                }
            }
        }

        // if we were unable to send messages to any clients, then remove them since the connection is probably now closed anyway
        for id in clients_to_remove {
            self.client_map.remove(&id);
        }
    }

    fn send_message(&mut self, connection_id: ConnectionId, message: ServerMessage) -> Result<(), ClientSendError> {
        match self.client_map.get_mut(&connection_id) {
            Some(client) => client.send(message),
            None => Err(ClientSendError(message))
        }
    }

    fn update_marker(&mut self, connection_id: ConnectionId, marker: FloEventId) {
        self.client_map.get_mut(&connection_id).map(|client| {
            client.update_marker(marker)
        });
    }
}
