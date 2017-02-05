#![feature(conservative_impl_trait)]
#![feature(collections_bound)]
#![feature(btree_range)]

extern crate flo_event;

#[macro_use]
extern crate nom;

/*
It's important that nom comes before log.
Both nom and log define an `error!` macro, and whichever one comes last wins.
Since we want to use the one from the log crate, that has to go last.
*/
#[macro_use]
extern crate log;

extern crate byteorder;
extern crate chrono;
extern crate num_cpus;

#[macro_use]
extern crate tokio_core;
extern crate futures;
extern crate glob;

#[cfg(test)]
extern crate env_logger;
#[cfg(test)]
extern crate tempdir;

pub mod client;
pub mod event_store;
pub mod protocol;
pub mod server;
pub mod time;
pub mod serializer;
pub mod error;
