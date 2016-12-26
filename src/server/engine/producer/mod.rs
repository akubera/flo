
use server::engine::api::{ConnectionId, ClientConnect, ProduceEvent, EventAck, ClientMessage, ServerMessage};

use event_store::StorageEngine;
use event_store::test_util::TestStorageEngine;
use flo_event::{ActorId, OwnedFloEvent, EventCounter, FloEventId};

use futures::sync::mpsc::UnboundedSender;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::path::PathBuf;
use std::net::SocketAddr;

struct ProducerMap(HashMap<ConnectionId, ClientConnect>);
impl ProducerMap {
    pub fn new() -> ProducerMap {
        ProducerMap(HashMap::with_capacity(32))
    }

    pub fn add(&mut self, client: ClientConnect) {
        let connection_id = client.connection_id;
        self.0.insert(connection_id, client);
    }

    pub fn remove(&mut self, client: ConnectionId) {
        self.0.remove(&client);
    }

    pub fn send(&mut self, client: ConnectionId, message: ServerMessage) -> Result<(), String> {
        self.0.get_mut(&client).ok_or_else(|| {
            format!("Client: {} does not exist in producer map", client)
        }).and_then(|client_connect| {
            client_connect.message_sender.send(message).map_err(|err| {
                format!("Failed to send to client: {}, addr: {:?}, err: {:?}", client, client_connect.client_addr, err)
            })
        })
    }
}

pub struct ProducerManager<S: StorageEngine> {
    actor_id: ActorId,
    event_store: S,
    highest_event_id: EventCounter,
    consumer_manager_channel: Sender<ClientMessage>,
    clients: ProducerMap,
}

impl <S: StorageEngine> ProducerManager<S> {
    pub fn new(storage: S, consumer_manager_channel: Sender<ClientMessage>, actor_id: ActorId, highest_event_id: EventCounter) -> ProducerManager<S> {
        ProducerManager {
            actor_id: actor_id,
            event_store: storage,
            highest_event_id: highest_event_id,
            consumer_manager_channel: consumer_manager_channel,
            clients: ProducerMap::new(),
        }
    }

    pub fn process(&mut self, client_message: ClientMessage) -> Result<(), String> {
        match client_message {
            ClientMessage::ClientConnect(client_connect) => {
                self.clients.add(client_connect);
                Ok(())
            }
            ClientMessage::Produce(produce_event) => {
                self.produce_event(produce_event)
            }
            msg @ _ => Err(format!("No ProducerManager handling for client message: {:?}", msg))
        }
    }

    fn produce_event(&mut self, event: ProduceEvent) -> Result<(), String> {
        let producer_id = event.connection_id;
        let op_id = event.op_id;
        let event_id = FloEventId::new(self.actor_id, self.highest_event_id);
        let owned_event = OwnedFloEvent {
            id: event_id,
            namespace: "whatever".to_owned(),
            data: event.event_data,
        };

        self.event_store.store(&owned_event).map(|()| {
            self.highest_event_id += 1;
            debug!("Stored event, new highest_event_id: {}", self.highest_event_id);

            let event_ack = ServerMessage::EventPersisted(EventAck {
                op_id: op_id,
                event_id: event_id,
            });
            self.clients.send(producer_id, event_ack);
        });
        Ok(())
    }
}


