extern crate flo;
extern crate flo_event;
extern crate url;
extern crate env_logger;
extern crate tempdir;
extern crate byteorder;

#[macro_use]
extern crate nom;

#[macro_use]
mod test_utils;

use test_utils::*;
use flo_event::{FloEventId, OwnedFloEvent, FloEvent};
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};
use std::net::{TcpStream, SocketAddr, SocketAddrV4, Ipv4Addr};
use std::io::{Write, Read};

use byteorder::{ByteOrder, BigEndian};
use nom::{be_u64, be_u32, be_u16};

integration_test!{event_is_written_and_ackgnowledged, _port, tcp_stream, {
    let event_data = b"ninechars";
    let op_id = produce_event(&mut tcp_stream, "/foo/bar", &event_data[..]);

    let mut buff = [0; 1024];
    let nread = tcp_stream.read(&mut buff).unwrap();

    let expected = b"FLO_ACK\n";
    assert_eq!(&expected[..], &buff[..8]);      //header
    let result_op_id = BigEndian::read_u32(&buff[8..12]);
    assert_eq!(op_id, result_op_id);
    assert_eq!(&[0, 0, 0, 0, 0, 0, 0, 1, 0, 1], &buff[12..22]);//event id (actor: u16, event_counter: u64)
}}

integration_test!{persisted_event_are_consumed_after_they_are_written, server_port, tcp_stream, {
    let event1_data = b"first event data";
    let first_namespace = "/first".to_owned();
    produce_event(&mut tcp_stream, &first_namespace, &event1_data[..]);

    let event2_data = b"second event data";
    let second_namespace = "/first".to_owned();
    produce_event(&mut tcp_stream, &second_namespace, &event2_data[..]);

    thread::sleep(Duration::from_millis(250));
    let mut buffer = [0; 128];
    let nread = tcp_stream.read(&mut buffer[..]).unwrap();
    assert!(nread > 0);

    let mut consumer = connect(server_port);
    consumer.write_all(b"FLO_CNS\n").unwrap();
    consumer.write_all(&[0, 0, 0, 0, 0, 0, 0, 2]).unwrap();
    thread::sleep(Duration::from_millis(250));

    let results = read_events(&mut consumer, 2);
    assert_eq!(event1_data, results[0].data());
    assert_eq!(first_namespace, results[0].namespace);
    assert_eq!(event2_data, results[1].data());
    assert_eq!(second_namespace, results[1].namespace);
}}

integration_test!{events_are_consumed_by_another_connection_as_they_are_written, server_port, tcp_stream, {
    let mut consumer = connect(server_port);
    consumer.write_all(b"FLO_CNS\n").unwrap();
    consumer.write_all(&[0, 0, 0, 0, 0, 0, 0, 2]).unwrap();

    let event1_data = b"first event data";
    produce_event(&mut tcp_stream, "/animal/pig", &event1_data[..]);

    let event2_data = b"second event data";
    produce_event(&mut tcp_stream, "/animal/donkey", &event2_data[..]);

    let results = read_events(&mut consumer, 2);
    assert_eq!(event1_data, results[0].data());
    assert_eq!(event2_data, results[1].data());
}}

///////////////////////////////////////////////////////////////////////////
///////  Test Utils            ////////////////////////////////////////////
///////////////////////////////////////////////////////////////////////////
///////////////////////////////////////////////////////////////////////////

named!{pub parse_str<String>,
    map_res!(
        take_until_and_consume!("\n"),
        |res| {
            ::std::str::from_utf8(res).map(|val| val.to_owned())
        }
    )
}

named!{parse_event<OwnedFloEvent>,
    chain!(
        _tag: tag!("FLO_EVT\n") ~
        actor: be_u16 ~
        counter: be_u64 ~
        namespace: parse_str ~
        data: length_bytes!(be_u32),
        || {
            OwnedFloEvent {
                id: FloEventId::new(actor, counter),
                namespace: namespace.to_owned(),
                data: data.to_owned()
            }
        }
    )
}

fn read_events(tcp_stream: &mut TcpStream, mut nevents: usize) -> Vec<OwnedFloEvent> {
    use nom::IResult;

    tcp_stream.set_read_timeout(Some(Duration::from_millis(1000))).unwrap();
    let mut events = Vec::new();
    let mut buffer_array = [0; 8 * 1024];
    let mut nread = tcp_stream.read(&mut buffer_array[..]).unwrap();
    let mut buffer_start = 0;
    let mut buffer_end = nread;
    while nevents > 0 {
        if buffer_start == buffer_end {
            println!("reading again");
            nread = tcp_stream.read(&mut buffer_array[..]).unwrap();
            buffer_end = nread;
            buffer_start = 0;
        }
        let buffer = &mut buffer_array[buffer_start..buffer_end];
        println!("attempting to read event: {}, buffer: {:?}", nevents, buffer);
        match parse_event(buffer) {
            IResult::Done(remaining, event) => {
                events.push(event);
                buffer_start += buffer.len() - remaining.len();
            }
            IResult::Error(err) => panic!("Error deserializing event: {:?}", err),
            IResult::Incomplete(need) => {
                panic!("Incomplete data to read events: {:?}, buffer: {:?}", need, buffer)
            }
        };
        nevents -= 1;
    }
    events
}

fn connect(port: u16) -> TcpStream {
    let address: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port));
    let stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(Duration::from_millis(1_000))).unwrap();
    stream
}

static mut OP_ID: AtomicUsize = ATOMIC_USIZE_INIT;

fn produce_event(tcp_stream: &mut TcpStream, namespace: &str, bytes: &[u8]) -> u32 {

    let op_id = unsafe {
        OP_ID.fetch_add(1, Ordering::SeqCst) as u32
    };
    tcp_stream.write_all(b"FLO_PRO\n").unwrap();
    tcp_stream.write_all(namespace.as_bytes()).unwrap();
    tcp_stream.write_all(b"\n").unwrap();
    let mut buffer = [0; 4];
    BigEndian::write_u32(&mut buffer[..], op_id);
    tcp_stream.write_all(&buffer).unwrap();

    BigEndian::write_u32(&mut buffer[..], bytes.len() as u32);
    tcp_stream.write_all(&buffer).unwrap();
    tcp_stream.write_all(bytes).unwrap();

    op_id
}


