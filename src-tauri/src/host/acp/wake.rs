//! Wakes the Tauri pump thread when an ACP inbound event is queued.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Debug)]
pub struct AdapterWake {
    flag: Mutex<bool>,
    cv: Condvar,
}

impl AdapterWake {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            flag: Mutex::new(false),
            cv: Condvar::new(),
        })
    }

    pub fn ping(&self) {
        if let Ok(mut flag) = self.flag.lock() {
            *flag = true;
            self.cv.notify_one();
        }
    }

    /// Wait until pinged, or `timeout`, whichever comes first.
    pub fn wait_timeout(&self, timeout: Duration) {
        let Ok(mut flag) = self.flag.lock() else {
            return;
        };
        if !*flag {
            flag = match self.cv.wait_timeout(flag, timeout) {
                Ok((g, _)) => g,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        *flag = false;
    }
}
