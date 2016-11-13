mod index;
mod file_reader;

use event::{EventId, Event};
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use lru_time_cache::LruCache;
use self::index::{RingIndex, Entry};
use self::file_reader::FileReader;

const EVENTS_FILE_NAME: &'static str = "events.json";
const MAX_CACHED_EVENTS: usize = 150;

pub type PersistenceResult = Result<EventId, io::Error>;

pub trait EventStore: Sized {
    fn create(base_dir: &Path, namespace: &str, max_num_events: usize) -> Result<Self, io::Error>;

    fn store(&mut self, event: Event) -> PersistenceResult;

    fn get_event_greater_than(&mut self, event_id: EventId) -> Option<&mut Event>;

    fn get_greatest_event_id(&self) -> EventId;
}


#[allow(dead_code)]
pub struct FileSystemEventStore {
    persistence_dir: PathBuf,
    events_file: File,
    index: RingIndex,
    event_cache: LruCache<EventId, Event>,
    current_file_position: u64,
    file_reader: FileReader,
    current_event_id: EventId,
}


impl FileSystemEventStore {
    pub fn new(persistence_dir: PathBuf, max_events: usize) -> Result<FileSystemEventStore, io::Error> {
        use std::fs;

        fs::create_dir_all(&persistence_dir)
            .and_then(|_| {
                fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(persistence_dir.join(EVENTS_FILE_NAME))
            })
            .and_then(|file| {
                let store = FileSystemEventStore {
                    file_reader: FileReader::new(persistence_dir.join(EVENTS_FILE_NAME)),
                    current_file_position: 0,
                    events_file: file,
                    index: RingIndex::new(1000, max_events),
                    event_cache: LruCache::<EventId, Event>::with_capacity(MAX_CACHED_EVENTS),
                    persistence_dir: persistence_dir,
                    current_event_id: 0,
                };

                store.initialize()
            })
    }

    fn initialize(mut self) -> Result<FileSystemEventStore, io::Error> {
        {
            let FileSystemEventStore { ref mut file_reader,
                                       ref mut event_cache,
                                       ref mut current_file_position,
                                       ref mut index,
                                       ref persistence_dir,
                                       ref mut current_event_id,
                                       .. } = self;


            let mut num_events = 0;
            for event_result in file_reader.read_from_offset(0) {
/*                match event_result {
                    Ok(mut event) => {
                        let event_data_len = event.get_raw_bytes().len() as u64;
                        let event_id = event.get_id();
                        index.add(Entry::new(event_id, event_data_len));
                        *current_file_position += event_data_len;
                        event_cache.insert(event_id, event);
                        num_events += 1;
                        *current_event_id = event_id;
                    }
                    Err(serde_error) => {
                        error!("Unable to Read Event: {:?}", serde_error);
                        match serde_error {
                            JsonError::Io(io_err) => return Err(io_err),
                            err @ _ => {
                                return Err(io::Error::new(io::ErrorKind::InvalidData,
                                                          format!("Error reading events file: \
                                                                   {:?}",
                                                                  err)))
                            }
                        }
                    }
                }*/
            }
            info!("initialized event store with {} pre-existing events from file: {:?}",
                  num_events,
                  persistence_dir);

        }

        Ok(self)
    }
}

impl EventStore for FileSystemEventStore {
    fn create(base_dir: &Path, namespace: &str, max_events: usize) -> Result<Self, io::Error> {
        let storage_dir = base_dir.join(namespace);
        FileSystemEventStore::new(storage_dir, max_events)
    }

    fn store(&mut self, event: Event) -> PersistenceResult {
        let mut evt = event;
        let event_id: EventId = evt.get_id();
        self.current_event_id = event_id;
        let dropped_entry = self.index.add(Entry::new(event_id, self.current_file_position));
        debug!("added index entry: event_id: {}, offset: {}, dropped entry: {:?}",
               event_id,
               self.current_file_position,
               dropped_entry);

        dropped_entry.map(|entry| self.event_cache.remove(&entry.event_id));
        self.events_file.write(evt.get_raw_bytes()).map(|_| {
            self.current_file_position += evt.get_raw_bytes().len() as u64;
            trace!("finished writing event to disk: {}", event_id);
            self.event_cache.insert(event_id, evt);
            event_id
        })
    }

    fn get_greatest_event_id(&self) -> EventId {
        self.current_event_id
    }

    fn get_event_greater_than(&mut self, event_id: EventId) -> Option<&mut Event> {
        let FileSystemEventStore { ref mut index, ref mut event_cache, ref file_reader, .. } = *self;

        index.get_next_entry(event_id).and_then(move |entry| {
            if event_cache.contains_key(&entry.event_id) {
                trace!("returning cached event: {}", event_id);
                event_cache.get_mut(&entry.event_id)
            } else {
                match file_reader.read_from_offset(entry.offset).next() {
                    Some(Ok(event)) => {
                        trace!("event cache miss for event: {}, read from file",
                               entry.event_id);
                        event_cache.insert(entry.event_id, event);
                        event_cache.get_mut(&entry.event_id)
                    }
                    _ => None,
                }
            }

        })
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use tempdir::TempDir;
    use event::{Event, EventId};
    use std::fs::File;
    use std::io::Read;
    use event_store::index::Entry;

    fn to_event(id: EventId, data: &str) -> Event {
        Event::new(id, data.as_bytes().to_owned())
    }

    #[test]
    fn once_the_maximum_number_of_events_is_exceeded_earlier_events_are_dropped() {
        let max_events = 50;
        let temp_dir = TempDir::new("flo-persist-test").unwrap();
        let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), max_events).unwrap();

        for i in 1..(max_events + 2) {
            let event = to_event(i as u64, r#"{"what":"evar"}"#);
            store.store(event).unwrap();
        }

        let result = store.get_event_greater_than(0).unwrap();
        assert_eq!(2, result.get_id());
    }

    #[test]
    fn new_event_store_is_created_with_existing_events_on_disk() {
        let temp_dir = TempDir::new("flo-persist-test").unwrap();
        let mut expected_event_1 = to_event(1, r#"{"firstEventKey":"firstEventValue"}"#);
        let mut expected_event_2 = to_event(2, r#"{"secondEventKey": "secondEventValue"}"#);

        {
            let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), 10).unwrap();
            store.store(expected_event_1.clone()).unwrap();
            store.store(expected_event_2.clone()).unwrap();
        }

        let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), 10).unwrap();
        {
            let actual_event_1 = store.get_event_greater_than(0);
            assert_eq!(Some(&mut expected_event_1), actual_event_1);
        }
        let actual_event_2 = store.get_event_greater_than(1);
        assert_eq!(Some(&mut expected_event_2), actual_event_2);
    }

    #[test]
    fn storing_an_event_adds_its_starting_offset_to_the_index() {
        let temp_dir = TempDir::new("flo-persist-test").unwrap();
        let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), 10).unwrap();

        let first_event_data = r#"{"id":1,"data":{"firstEventKey":"firstEventValue"}}"#;

        let event = to_event(1, "firstEventData");
        store.store(event).unwrap();
        let event = to_event(2, "secondEventValue");
        store.store(event).unwrap();

        let index_entry = store.index.get(2).unwrap();
        let expected_offset = first_event_data.as_bytes().len() as u64;
        assert_eq!(expected_offset, index_entry.offset);
    }

    #[test]
    fn storing_an_event_adds_its_event_id_to_the_index() {
        let temp_dir = TempDir::new("flo-persist-test").unwrap();
        let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), 10).unwrap();

        let event_id: EventId = 3;
        let event = to_event(event_id, r#"{"myKey": "myValue"}"#);

        store.store(event).unwrap();

        let index_entry = store.index.get(event_id);
        assert!(index_entry.is_some());
        assert_eq!(index_entry.unwrap().offset, 0); // first entry written to file
    }

    #[test]
    fn get_next_event_returns_a_non_cached_event_stored_on_disk() {
        let temp_dir = TempDir::new("flo-persist-test").unwrap();
        let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), 20).unwrap();

        for i in 1..11 {
            let event = Event::new(i, format!("event: {}", i).into_bytes());
            store.store(event).unwrap();
            store.event_cache.remove(&i);
        }
        assert!(store.event_cache.is_empty());
        let result = store.get_event_greater_than(5);
        assert!(result.is_some());
        assert_eq!(6, result.unwrap().get_id());
    }

    #[test]
    fn get_next_event_returns_a_cached_event() {
        let temp_dir = TempDir::new("flo-persist-test").unwrap();
        let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), 10).unwrap();
        let event_id: EventId = 3;
        let mut event = to_event(event_id, r#"{"myKey": "one"}"#);

        store.event_cache.insert(event_id, event.clone());
        store.index.add(Entry::new(event_id, 9876));

        let result = store.get_event_greater_than(event_id - 1);
        assert_eq!(Some(&mut event), result);
    }

    #[test]
    fn events_are_put_into_the_cache_when_they_are_stored() {
        let temp_dir = TempDir::new("flo-persist-test").unwrap();
        let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), 10).unwrap();

        let event = to_event(1, r#"{"myKey": "one"}"#);
        store.store(event.clone()).unwrap();
        assert_eq!(Some(&event), store.event_cache.get(&1));
    }

    #[test]
    fn multiple_events_are_saved_sequentially() {
        let temp_dir = TempDir::new("flo-persist-test").unwrap();
        let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), 10).unwrap();

        let event1 = to_event(1, r#"{"myKey": "one"}"#);
        store.store(event1.clone()).unwrap();

        let event2 = to_event(2, r#"{"myKey": "one"}"#);
        store.store(event2.clone()).unwrap();

        let file_path = temp_dir.path().join("events.json");
        let file = File::open(file_path).unwrap();

        let bytes = file.bytes();

        panic!("still need to finish test");
    }

    #[test]
    fn event_is_saved_to_a_file_within_the_persistence_directory() {
        let temp_dir = TempDir::new("flo-persist-test").unwrap();
        let mut store = FileSystemEventStore::new(temp_dir.path().to_path_buf(), 10).unwrap();

        let event = to_event(1, r#"{"myKey": "myVal"}"#);
        store.store(event.clone()).unwrap();

        let file_path = temp_dir.path().join("events.json");
        let mut file = File::open(file_path).unwrap();

        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        panic!("still need to finish test");
    }
}
