use std::net::SocketAddr;

use server::event_loops::LoopHandles;
use server::engine::api::{next_connection_id, ProducerMessage, ClientMessage};
use super::setup_message_streams;
use server::channel_sender::ChannelSender;

use tokio_core::reactor::{Remote, Handle};
use tokio_core::net::TcpStream;
use futures::{Future, Stream};
use futures::sync::mpsc::UnboundedReceiver;


fn connect(address: SocketAddr, handle: &Handle, remote: Remote, engine: ChannelSender) -> impl Future<Item=(), Error=()> + 'static {
    TcpStream::connect(&address, handle).then(move |result| {
        match result {
            Ok(tcp_stream) => {
                let connection_id = next_connection_id();
                debug!("established connection to peer at: {} with connection_id: {}", address, connection_id);
                setup_message_streams(connection_id, tcp_stream, address, engine.clone(), &remote);
                let peer_connect_msg = ClientMessage::Producer(ProducerMessage::PeerConnectSuccess(connection_id, address));
                engine.send(peer_connect_msg).map_err(|send_err| {
                    error!("Error sending peer connect to engine: {:?}", send_err); // map err to ()
                })
            }
            Err(io_err) => {
                debug!("Failed to connect to peer address: {:?}, err: {}", address, io_err);
                let fail = ProducerMessage::PeerConnectFailed(address);
                engine.send(ClientMessage::Producer(fail)).expect("Failed to send PeerConnectFailed message to producer manager");
                Err(())
            }
        }
    })
}

//TODO: setup Interval events for handling timeouts
pub fn start_cluster_io(receiver: UnboundedReceiver<SocketAddr>, mut loop_handles: LoopHandles, engine: ChannelSender) {
    loop_handles.next_handle().spawn(move |_| {
        receiver.for_each(move |peer_address| {
            let engine = engine.clone();
            let remote = loop_handles.next_handle();
            let address = peer_address.clone();
            loop_handles.next_handle().spawn(move |handle| {
                connect(address, handle, remote, engine)
            });
            Ok(())
        })
    });
}
