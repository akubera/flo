use std::cmp::{Ord, PartialOrd, Ordering};
use std::collections::HashMap;


pub type ActorId = u16;
pub type EventCounter = u64;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FloEventId {
    pub actor: ActorId,
    pub event_counter: EventCounter,
}

impl FloEventId {
    pub fn new(actor: ActorId, event_counter: EventCounter) -> FloEventId {
        FloEventId {
            event_counter: event_counter,
            actor: actor,
        }
    }
}

impl Ord for FloEventId {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.event_counter == other.event_counter {
            self.actor.cmp(&other.actor)
        } else {
            self.event_counter.cmp(&other.event_counter)
        }
    }
}

impl PartialOrd for FloEventId {
    fn partial_cmp(&self, other: &FloEventId) -> Option<Ordering> {
        if self.event_counter == other.event_counter {
            self.actor.partial_cmp(&other.actor)
        } else {
            self.event_counter.partial_cmp(&other.event_counter)
        }
    }
}

pub trait FloEventIdMap {
    fn new() -> Self;
    fn get_counter(&self, actor: ActorId) -> EventCounter;
    fn increment(&mut self, actor: ActorId, amount: u64) -> EventCounter;

    fn set(&mut self, event_id: FloEventId);

    fn event_is_greater(&self, event_id: FloEventId) -> bool {
        let current = self.get_counter(event_id.actor);
        event_id.event_counter > current
    }
}


impl FloEventIdMap for HashMap<ActorId, EventCounter> {
    fn new() -> HashMap<ActorId, EventCounter> {
        HashMap::new()
    }

    fn get_counter(&self, actor: ActorId) -> EventCounter {
        self.get(&actor).map(|c| *c).unwrap_or(0)
    }

    fn increment(&mut self, actor: ActorId, amount: u64) -> EventCounter {
        let current: &mut EventCounter = self.entry(actor).or_insert(0);
        *current += amount;
        *current
    }

    fn set(&mut self, event_id: FloEventId) {
        let actor = event_id.actor;
        let counter: &mut EventCounter = self.entry(actor).or_insert(event_id.event_counter);
        *counter = event_id.event_counter
    }

}


pub trait FloEvent {
    fn id(&self) -> &FloEventId;
    fn namespace(&self) -> &str;
    fn data_len(&self) -> u32;
    fn data(&self) -> &[u8];

    fn to_owned(&self) -> OwnedFloEvent;
}

#[derive(Debug, PartialEq, Clone)]
pub struct OwnedFloEvent {
    pub id: FloEventId,
    pub namespace: String,
    pub data: Vec<u8>,
}

impl OwnedFloEvent {
    pub fn new(id: FloEventId, namespace: String, data: Vec<u8>) -> OwnedFloEvent {
        OwnedFloEvent {
            id: id,
            namespace: namespace,
            data: data,
        }
    }
}

impl FloEvent for OwnedFloEvent {
    fn id(&self) -> &FloEventId {
        &self.id
    }

    fn namespace(&self) -> &str {
        &self.namespace
    }

    fn data_len(&self) -> u32 {
        self.data.len() as u32
    }

    fn data(&self) -> &[u8] {
        &self.data
    }

    fn to_owned(&self) -> OwnedFloEvent {
        self.clone()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn flo_event_id_map_has_current_value_set() {
        let mut map = HashMap::new();

        map.set(FloEventId::new(2, 33));

        assert_eq!(33, map.get_counter(2));
    }

    #[test]
    fn event_is_greater_returns_true_if_event_id_is_greater_than_current() {
        let mut map = HashMap::new();

        map.set(FloEventId::new(2, 33));

        assert!(map.event_is_greater(FloEventId::new(2, 34)));
    }

    #[test]
    fn event_is_greater_returns_false_when_event_id_equals_current() {
        let mut map = HashMap::new();

        map.set(FloEventId::new(2, 33));

        assert!(!map.event_is_greater(FloEventId::new(2, 33)));
    }

    #[test]
    fn event_is_greater_returns_false_when_event_id_is_less_than_current() {
        let mut map = HashMap::new();

        map.set(FloEventId::new(2, 33));

        assert!(!map.event_is_greater(FloEventId::new(2, 32)));
    }

    #[test]
    fn event_is_greater_returns_true_if_actor_is_not_represented_in_map() {
        let mut map = HashMap::new();

        assert!(map.event_is_greater(FloEventId::new(2, 1)));
    }


}
