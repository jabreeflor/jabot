//! Per-thread monotonic sequence numbers for host → client envelopes.

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct SeqStore {
    heads: HashMap<String, u64>,
}

impl SeqStore {
    pub fn next(&mut self, thread_id: &str) -> u64 {
        let head = self.heads.entry(thread_id.to_string()).or_insert(0);
        *head += 1;
        *head
    }

    pub fn head(&self, thread_id: &str) -> u64 {
        self.heads.get(thread_id).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_is_per_thread_and_monotonic() {
        let mut seq = SeqStore::default();
        assert_eq!(seq.next("a"), 1);
        assert_eq!(seq.next("a"), 2);
        assert_eq!(seq.next("b"), 1);
        assert_eq!(seq.head("a"), 2);
        assert_eq!(seq.head("missing"), 0);
    }
}
