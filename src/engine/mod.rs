pub mod api;
pub mod event_store;
pub mod version_vec;

mod producer;
mod consumer;

use self::api::{ConsumerMessage, ProducerMessage};
use self::producer::ProducerManager;
use self::consumer::ConsumerManager;
use server::ServerOptions;
use engine::event_store::{StorageEngine, EventReader, StorageEngineOptions};
use engine::event_store::fs::{FSStorageEngine};
use event::ActorId;

use futures::sync::mpsc::UnboundedSender;

use std::net::SocketAddr;
use std::sync::mpsc;
use std::thread;

pub struct BackendChannels {
    pub producer_manager: mpsc::Sender<ProducerMessage>,
    pub consumer_manager: mpsc::Sender<ConsumerMessage>,
}

//TODO: use the cluster sender to setup peer connections
pub fn run(options: ServerOptions, cluster_sender: UnboundedSender<SocketAddr>) -> BackendChannels {
    let (producer_tx, producer_rx) = mpsc::channel::<ProducerMessage>();
    let (consumer_tx, consumer_rx) = mpsc::channel::<ConsumerMessage>();

    let ServerOptions{data_dir, default_namespace, max_events, max_cached_events, max_cache_memory, ..} = options;

    let storage_options = StorageEngineOptions {
        storage_dir: data_dir,
        root_namespace: default_namespace,
        max_events: max_events,
    };

    //TODO: set max events and namespace and have some proper error handling
    let actor_id: ActorId = 1;
    let (event_writer, mut event_reader, version_vec) = FSStorageEngine::initialize(storage_options).expect("Failed to initialize storage engine");
    let highest_event_id = event_reader.get_highest_event_id();


    let consumer_manager_sender = consumer_tx.clone();
    thread::Builder::new().name("Producer-Manager-thread".to_owned()).spawn(move || {
        let mut producer_manager = ProducerManager::new(event_writer, consumer_manager_sender, actor_id, version_vec);
        loop {
            match producer_rx.recv() {
                Ok(msg) => {
                    match producer_manager.process(msg) {
                        Ok(()) => {
                            trace!("Producer Manager successfully processed message");
                        }
                        Err(err) => {
                            error!("ProducerManager error processing message err: {:?}", err)
                        }
                    }
                }
                Err(recv_err) => {
                    error!("Receive Error: {:?}\nShutting down ProducerManager", recv_err);
                    break;
                }
            }
        }
    }).expect("Failed to start Producer Manager thread");

    let consumer_manager_sender = consumer_tx.clone();
    thread::Builder::new().name("Consumer-Manager-thread".to_owned()).spawn(move || {
        let mut consumer_manager = ConsumerManager::new(event_reader, consumer_manager_sender, highest_event_id, max_cached_events, max_cache_memory);

        loop {
            match consumer_rx.recv() {
                Ok(client_message) => {
                    match consumer_manager.process(client_message) {
                        Ok(()) => {
                            trace!("Consumer manager succesfully processed message");
                        }
                        Err(err) => {
                            error!("ConsumerManager error in processing message err: {:?}", err);
                        }
                    }
                }
                Err(err) => {
                    error!("Receive Error: {:?}\nshutting down Consumer Manager", err);
                    break;
                }
            }
        }
    }).expect("Failed to start producer manager thread");

    BackendChannels {
        producer_manager: producer_tx,
        consumer_manager: consumer_tx.clone(),
    }
}

