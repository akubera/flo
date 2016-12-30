use std::sync::{Arc, RwLock};
use std::io::{self, Seek, SeekFrom, Read, BufRead, BufReader};
use std::path::{PathBuf, Path};
use std::fs::{OpenOptions, File};

use byteorder::{ByteOrder, BigEndian};

use event_store::index::{EventIndex, IndexEntry};
use event_store::{EventReader, StorageEngineOptions};
use flo_event::{FloEventId, ActorId, EventCounter, OwnedFloEvent, FloEvent};

enum EventIterInner {
    Empty,
    NonEmpty{
        file_reader: BufReader<File>,
        remaining: usize,
    },
    Error(Option<io::Error>)
}

pub struct FSEventIter(EventIterInner);

impl FSEventIter {
    pub fn initialize(offset: u64, limit: usize, path: &Path) -> Result<FSEventIter, io::Error> {
        trace!("opening file at path: {:?}", path);
        OpenOptions::new().read(true).open(path).and_then(|mut file| {
            file.seek(SeekFrom::Start(offset)).map(|_| {
                let inner = EventIterInner::NonEmpty {
                    file_reader: BufReader::new(file),
                    remaining: limit,
                };
                FSEventIter(inner)
            })
        })
    }

    pub fn empty() -> FSEventIter {
        FSEventIter(EventIterInner::Empty)
    }

    fn error(err: io::Error) -> FSEventIter {
        FSEventIter(EventIterInner::Error(Some(err)))
    }
}

impl Iterator for FSEventIter {
    type Item = Result<OwnedFloEvent, io::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0 {
            EventIterInner::Empty => None,
            EventIterInner::Error(ref mut err) => {
                err.take().map(|e| {
                    Err(e)
                })
            }
            EventIterInner::NonEmpty {ref mut file_reader, ref mut remaining} => {
                if *remaining == 0 {
                    None
                } else {
                    *remaining -= 1;
                    match has_next_event(file_reader) {
                        Ok(true) => {
                            Some(read_event(file_reader))
                        }
                        Ok(false) => {
                            None
                        }
                        Err(err) => {
                            Some(Err(err))
                        }
                    }
                }
            }
        }
    }
}

fn has_next_event<R: BufRead>(reader: &mut R) -> Result<bool, io::Error> {
    reader.fill_buf().map(|buffer| {
        buffer.len() >= 8 && &buffer[..8] == super::FLO_EVT.as_bytes()
    })
}

pub struct EventHeader {
    pub total_size: u32,
    pub event_counter: u64,
    pub actor_id: u16,
    pub namespace_length: u32,
}

impl EventHeader {
    pub fn size_on_disk() -> usize {
        26
    }

    pub fn compute_data_length(&self) -> usize {
        (self.total_size - 4 - 10 - 4 - self.namespace_length - 4) as usize
    }

    pub fn event_id(&self) -> FloEventId {
        FloEventId::new(self.actor_id, self.event_counter)
    }
}

pub fn read_header<R: Read>(reader: &mut R) -> Result<EventHeader, io::Error> {
    let mut buffer = [0; 26];
    reader.read_exact(&mut buffer[..])?;

    if &buffer[..8] == super::FLO_EVT.as_bytes() {
        Ok(EventHeader{
            total_size: BigEndian::read_u32(&buffer[8..12]),
            event_counter: BigEndian::read_u64(&buffer[12..20]),
            actor_id: BigEndian::read_u16(&buffer[20..22]),
            namespace_length: BigEndian::read_u32(&buffer[22..26]),
        })
    } else {
        Err(invalid_bytes_err(format!("expected {:?}, got: {:?}", super::FLO_EVT.as_bytes(), &buffer[..8])))
    }
}

pub fn read_event<R: Read>(reader: &mut R) -> Result<OwnedFloEvent, io::Error> {
    read_header(reader).and_then(|header| {
        let mut namespace_buffer = vec![0; header.namespace_length as usize];
        reader.read_exact(&mut namespace_buffer).and_then(move |ns_read| {
            String::from_utf8(namespace_buffer).map_err(|err| {
                invalid_bytes_err(format!("namespace contained invalid utf8 character: {:?}", err))
            })
        }).and_then(|namespace| {
            let mut data_len_buffer = [0; 4];
            reader.read_exact(&mut data_len_buffer).and_then(|()| {
                let data_length = BigEndian::read_u32(&data_len_buffer);
                debug_assert_eq!(data_length as usize, header.compute_data_length());
                let mut data_buffer = vec![0; data_length as usize];
                reader.read_exact(&mut data_buffer).map(|()| {
                    OwnedFloEvent::new(header.event_id(), namespace, data_buffer)
                })
            })
        })
    })
}

fn invalid_bytes_err(error_desc: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error_desc)
}

pub struct FSEventReader {
    index: Arc<RwLock<EventIndex>>,
    storage_dir: PathBuf,
}

impl FSEventReader {
    pub fn initialize(index: Arc<RwLock<EventIndex>>, options: &StorageEngineOptions) -> Result<FSEventReader, io::Error> {
        let storage_dir = options.storage_dir.as_path().join(&options.root_namespace);
        Ok(FSEventReader{
            index: index,
            storage_dir: storage_dir,
        })
    }

    /// A bit of future proofing here, since index entry really isn't needed yet
    fn get_events_file(&self, entry: &IndexEntry) -> PathBuf {
        self.storage_dir.as_path().join(super::DATA_FILE_NAME)
    }
}

impl EventReader for FSEventReader {
    type Iter = FSEventIter;
    type Error = io::Error;

    fn load_range(&mut self, range_start: FloEventId, limit: usize) -> Self::Iter {
        let index = self.index.read().expect("Unable to acquire read lock on event index");

        index.get_next_entry(range_start).map(|entry| {
            let event_file = self.get_events_file(&entry);
            trace!("range_start: {:?}, next_entry: {:?}, file: {:?}", range_start, entry, event_file);
            FSEventIter::initialize(entry.offset, limit, &event_file).unwrap_or_else(|err| {
                error!("Unable to create event iterator: {:?}", err);
                FSEventIter::error(err)
            })
        }).unwrap_or_else(|| {
            trace!("Creating an empty iterator in response to range_start: {:?}", range_start);
            FSEventIter::empty()
        })
    }

    fn get_highest_event_id(&mut self) -> FloEventId {
        let index = self.index.read().expect("Unable to acquire read lock on event index");
        index.get_greatest_event_id()
    }
}
