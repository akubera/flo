#![feature(conservative_impl_trait)]
#![feature(collections_bound)]
#![feature(btree_range)]

extern crate clap;

#[macro_use]
extern crate nom;
/*
It's important that nom comes before log.
Both nom and log define an `error!` macro, and whichever one comes last wins.
Since we want to use the one from the log crate, that has to go last.
*/
#[macro_use]
extern crate log;

extern crate log4rs;
extern crate num_cpus;
extern crate byteorder;
extern crate tokio_core;
extern crate futures;
extern crate chrono;
extern crate glob;

#[cfg(test)]
extern crate env_logger;
#[cfg(test)]
extern crate tempdir;

mod logging;
mod server;
mod engine;
mod time;
mod protocol;
mod serializer;
mod event;
mod channels;

use logging::{init_logging, LogLevelOption, LogFileOption};
use clap::{App, Arg, ArgMatches};
use std::str::FromStr;
use std::path::{PathBuf, Path};
use server::{ServerOptions, MemoryLimit, MemoryUnit};
use std::net::{SocketAddr, ToSocketAddrs};

const FLO_VERSION: &'static str = env!("CARGO_PKG_VERSION");

fn app_args() -> App<'static, 'static> {
    App::new("flo")
            .version(FLO_VERSION)
            .arg(Arg::with_name("log-level")
                    .short("-L")
                    .long("log")
                    .takes_value(true)
                    .value_name("module=level")
                    .multiple(true)
                    .help("Sets the log level for a module. Argument should be in the format module::sub-module=<level> where level is one of trace, debug, info, warn"))
            .arg(Arg::with_name("log-dest")
                    .long("log-dest")
                    .value_name("path")
                    .help("Path of a file to write logs to. Default is to log to stdout if unspecified"))
            .arg(Arg::with_name("port")
                    .short("p")
                    .long("port")
                    .value_name("PORT")
                    .help("port that the server should listen on")
                    .default_value("3000"))
            .arg(Arg::with_name("data-dir")
                    .short("d")
                    .long("data-dir")
                    .value_name("DIR")
                    .help("The directory to be used for storage")
                    .default_value("."))
            .arg(Arg::with_name("default-namespace")
                    .long("default-namespace")
                    .value_name("ns")
                    .help("Name of the default namespace")
                    .default_value("default"))
            .arg(Arg::with_name("max-events")
                    .long("max-events")
                    .value_name("max")
                    .help("Maximum number of events to keep, if left unspecified, defaults to max u32 or u64 depending on architecture"))
            .arg(Arg::with_name("max-cached-events")
                    .long("max-cached-events")
                    .value_name("max")
                    .help("Maximum number of events to cache in memory, if left unspecified, defaults to max u32 or u64 depending on architecture"))
            .arg(Arg::with_name("max-cache-memory")
                    .short("M")
                    .long("max-cache-memory")
                    .value_name("megabytes")
                    .default_value("512")
                    .help("Maximum amount of memory in megabytes to use for the event cache"))
            .arg(Arg::with_name("join-cluster-address")
                    .long("cluster-addr")
                    .short("c")
                    .multiple(true)
                    .value_name("HOST:PORT")
                    .help("address of another Flo instance to join a cluster; this argument may be supplied multiple times"))
}

fn main() {
    let args = app_args().get_matches();

    let log_levels = get_log_level_options(&args);
    let log_dest = get_log_file_option(&args);
    init_logging(log_dest, log_levels);

    let port = parse_arg_or_exit(&args, "port", 3000u16);
    let data_dir = PathBuf::from(args.value_of("data-dir").unwrap_or("."));
    let max_events = parse_arg_or_exit(&args, "max-events", ::std::usize::MAX);
    let default_ns = args.value_of("default-namespace").map(|value| value.to_owned()).expect("Must have a value for 'default-namespace' argument");
    let max_cached_events = parse_arg_or_exit(&args, "max-cached-events", ::std::usize::MAX);
    let max_cache_memory = get_max_cache_mem_amount(&args);
    let cluster_addresses = get_cluster_addresses(&args);

    let server_options = ServerOptions {
        default_namespace: default_ns,
        max_events: max_events,
        port: port,
        data_dir: data_dir,
        max_cached_events: max_cached_events,
        max_cache_memory: max_cache_memory,
        cluster_addresses: cluster_addresses,
    };
    server::run(server_options);
    info!("Shutdown server");
}

fn get_cluster_addresses(args: &ArgMatches) -> Option<Vec<SocketAddr>> {
    args.values_of("join-cluster-address").map(|values| {
        values.flat_map(|address_arg| {
            address_arg.to_socket_addrs()
                    .map_err(|err| {
                        format!("Unable to resolve address: '{}', error: {}", address_arg, err)
                    })
                    .or_bail()
                    .next()
        }).collect()
    })
}

fn get_log_file_option(args: &ArgMatches) -> LogFileOption {
    args.value_of("log-dest").map(|path| {
        LogFileOption::File(Path::new(path).to_path_buf())
    }).unwrap_or(LogFileOption::Stdout)
}

fn get_log_level_options(args: &ArgMatches) -> Vec<LogLevelOption> {
    args.values_of("log-level").map(|level_strs| {
        level_strs.map(|arg_value| {
            LogLevelOption::from_str(arg_value).or_bail()
        }).collect()
    }).unwrap_or(Vec::new())
}

fn get_max_cache_mem_amount(args: &ArgMatches) -> MemoryLimit {
    let mb = parse_arg_or_exit(args, "max-cache-memory", 512usize);
    MemoryLimit::new(mb, MemoryUnit::Megabyte)
}

fn parse_arg_or_exit<T: FromStr + Default>(args: &ArgMatches, arg_name: &str, default: T) -> T {
    args.value_of(arg_name)
        .map(|value| {
            value.parse::<T>().map_err(|_err| {
                format!("argument {} invalid value: {}", arg_name, value)
            }).or_bail()
        })
        .unwrap_or(default)
}

trait ParseArg<T> {
    fn or_bail(self) -> T;
}

impl <T> ParseArg<T> for Result<T, String> {
    fn or_bail(self) -> T {
        match self {
            Ok(value) => value,
            Err(err) => {
                println!("Error: {}", err);
                app_args().print_help().expect("failed to print help message");
                ::std::process::exit(1);
            }
        }
    }
}
