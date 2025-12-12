use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct Spinner {
    states: Vec<String>,
    interval: Duration,
    inner: Mutex<Inner>,
}

struct Inner {
    current_index: usize,
    last_trigger: Instant,
}

impl Default for Spinner {
    fn default() -> Self {
        Self {
            states: ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>(),
            interval: Duration::from_millis(100),
            inner: Mutex::new(Inner {
                current_index: 0,
                last_trigger: Instant::now(),
            }),
        }
    }
}

impl Spinner {
    pub fn get(&self) -> String {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();

        if now.duration_since(inner.last_trigger) >= self.interval {
            inner.last_trigger = now;
            inner.current_index = (inner.current_index + 1) % self.states.len();
        }

        self.states[inner.current_index].clone()
    }
}
