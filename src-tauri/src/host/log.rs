//! In-memory per-thread notification log so a reconnecting client can
//! `sync/resumeFrom` without waiting on ACP HTTP resume IDs.

use std::collections::HashMap;

use super::protocol::LoggedEvent;

const DEFAULT_CAP_PER_THREAD: usize = 4096;

#[derive(Debug)]
pub struct EventLog {
    events: HashMap<String, Vec<LoggedEvent>>,
    cap: usize,
}

impl Default for EventLog {
    fn default() -> Self {
        Self {
            events: HashMap::new(),
            cap: DEFAULT_CAP_PER_THREAD,
        }
    }
}

impl EventLog {
    pub fn push(&mut self, thread_id: &str, event: LoggedEvent) {
        let bucket = self.events.entry(thread_id.to_string()).or_default();
        bucket.push(event);
        if bucket.len() > self.cap {
            let overflow = bucket.len() - self.cap;
            bucket.drain(..overflow);
        }
    }

    pub fn after(&self, thread_id: &str, seq: u64) -> Vec<LoggedEvent> {
        self.events
            .get(thread_id)
            .map(|events| {
                events
                    .iter()
                    .filter(|event| event.seq > seq)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(seq: u64) -> LoggedEvent {
        LoggedEvent {
            seq,
            method: "session/update".into(),
            params: json!({ "seq": seq }),
        }
    }

    #[test]
    fn after_is_exclusive() {
        let mut log = EventLog::default();
        log.push("t", event(1));
        log.push("t", event(2));
        log.push("t", event(3));
        let replay = log.after("t", 1);
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].seq, 2);
        assert_eq!(replay[1].seq, 3);
    }

    #[test]
    fn cap_drops_oldest() {
        let mut log = EventLog {
            events: HashMap::new(),
            cap: 2,
        };
        log.push("t", event(1));
        log.push("t", event(2));
        log.push("t", event(3));
        let all = log.after("t", 0);
        assert_eq!(all.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![2, 3]);
    }
}
