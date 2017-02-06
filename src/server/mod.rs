pub mod engine;
pub mod metrics;
mod flo_io;
mod channel_sender;
mod event_loops;

use futures::stream::Stream;
use futures::{Future};
use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use tokio_core::reactor::Remote;
use tokio_core::net::{TcpStream, TcpListener};
use tokio_core::io as nio;
use tokio_core::io::Io;

use self::channel_sender::ChannelSender;
use protocol::{ClientProtocolImpl, ServerProtocolImpl};
use server::engine::api::{self, ClientMessage, ProducerMessage, ConsumerMessage, ClientConnect};
use protocol::ServerMessage;
use server::flo_io::{ClientMessageStream, ServerMessageStream};
use server::engine::BackendChannels;

use std::path::PathBuf;
use std::net::{SocketAddr, Ipv4Addr, SocketAddrV4};

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum MemoryUnit {
    Megabyte,
    Kilobyte,
    Byte
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct MemoryLimit {
    amount: usize,
    unit: MemoryUnit,
}


impl MemoryLimit {
    pub fn new(amount: usize, unit: MemoryUnit) -> MemoryLimit {
        MemoryLimit {
            amount: amount,
            unit: unit,
        }
    }

    pub fn as_bytes(&self) -> usize {
        let multiplier = match self.unit {
            MemoryUnit::Byte => 1,
            MemoryUnit::Kilobyte => 1024,
            MemoryUnit::Megabyte => 1024 * 1024,
        };
        multiplier * self.amount
    }
}

#[derive(PartialEq, Clone)]
pub struct ServerOptions {
    pub port: u16,
    pub data_dir: PathBuf,
    pub default_namespace: String,
    pub max_events: usize,
    pub max_cached_events: usize,
    pub max_cache_memory: MemoryLimit,
    pub cluster_addresses: Option<Vec<SocketAddr>>,
}

pub fn setup_message_streams(tcp_stream: TcpStream, client_addr: SocketAddr, engine: ChannelSender, remote_handle: &Remote) {

    remote_handle.spawn(move |_handle| {

        debug!("Got new connection from: {:?}", client_addr);
        let (server_tx, server_rx): (UnboundedSender<ServerMessage>, UnboundedReceiver<ServerMessage>) = unbounded();

        let connection_id = api::next_connection_id();
        let (tcp_reader, tcp_writer) = tcp_stream.split();

        let client_connect = ClientConnect {
            connection_id: connection_id,
            client_addr: client_addr,
            message_sender: server_tx.clone(),
        };

        engine.send(ClientMessage::Both(
            ConsumerMessage::ClientConnect(client_connect.clone()),
            ProducerMessage::ClientConnect(client_connect)
        )).unwrap(); //TODO: something better than unwrapping this result


        let client_stream = ClientMessageStream::new(connection_id, tcp_reader, ClientProtocolImpl);
        let client_to_server = client_stream.map_err(|err| {
            format!("Error parsing client stream: {:?}", err)
        }).and_then(move |client_message| {
            let log = format!("Sent message: {:?}", client_message);
            engine.send(client_message).map_err(|err| {
                format!("Error sending message: {:?}", err)
            }).map(|()| {
                log
            })
        }).for_each(|inner_thing| {
            info!("for each inner thingy: {:?}", inner_thing);
            Ok(())
        }).or_else(|err| {
            warn!("Recovering from error: {:?}", err);
            Ok(())
        });

        let server_to_client = nio::copy(ServerMessageStream::<ServerProtocolImpl>::new(connection_id, server_rx), tcp_writer).map_err(|err| {
            error!("Error writing to client: {:?}", err);
            format!("Error writing to client: {:?}", err)
        }).map(move |amount| {
            info!("Wrote: {} bytes to client: {:?}, connection_id: {}, dropping connection", amount, client_addr, connection_id);
            ()
        });

        client_to_server.select(server_to_client).then(move |res| {
            match res {
                Ok((compl, _fut)) => {
                    info!("Finished with connection: {}, value: {:?}", connection_id, compl);
                }
                Err((err, _)) => {
                    warn!("Error with connection: {}, err: {:?}", connection_id, err);
                }
            }
            Ok(())
        })
    });
}

pub fn run(options: ServerOptions) {
    let (join_handle, mut event_loop_handles) = self::event_loops::spawn_default_event_loops().unwrap();

    let server_port = options.port;
    let address: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), server_port));

    let BackendChannels{producer_manager, consumer_manager} = engine::run(options);

    event_loop_handles.next_handle().spawn(move |handle| {
        let listener = TcpListener::bind(&address, &handle).unwrap();

        info!("Started listening on port: {}", server_port);

        listener.incoming().map_err(|io_err| {
            error!("Error creating new connection: {:?}", io_err);
        }).for_each(move |(tcp_stream, client_addr): (TcpStream, SocketAddr)| {
            let channel_sender = ChannelSender {
                producer_manager: producer_manager.clone(),
                consumer_manager: consumer_manager.clone(),
            };

            let remote_handle = event_loop_handles.next_handle();
            setup_message_streams(tcp_stream, client_addr, channel_sender, &remote_handle);
            Ok(())
        })
    });

    join_handle.join();
}

