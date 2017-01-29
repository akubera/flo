pub mod sync;

use std::io;
use protocol::{ProtocolMessage, ErrorMessage};
use flo_event::{FloEventId};


#[derive(Debug)]
pub enum ClientError {
    Io(io::Error),
    FloError(ErrorMessage),
    UnexpectedMessage(ProtocolMessage),
}

impl From<io::Error> for ClientError {
    fn from(err: io::Error) -> Self {
        ClientError::Io(err)
    }
}

impl From<ErrorMessage> for ClientError {
    fn from(err: ErrorMessage) -> Self {
        ClientError::FloError(err)
    }
}


#[derive(Debug, PartialEq, Clone)]
pub struct ConsumerOptions {
    pub namespace: String,
    pub start_position: Option<FloEventId>,
    pub max_events: u64,
    pub username: String,
    pub password: String,
}



