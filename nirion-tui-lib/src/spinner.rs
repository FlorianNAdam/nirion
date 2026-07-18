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

#[cfg(test)]
mod tests {
    use super::*;

    fn spinner(
        interval: Duration,
        current_index: usize,
    ) -> Spinner {
        Spinner {
            states: vec!["a".into(), "b".into(), "c".into()],
            interval,
            inner: Mutex::new(Inner {
                current_index,
                last_trigger: Instant::now(),
            }),
        }
    }

    #[test]
    fn get_keeps_current_state_before_interval_elapses() {
        let spinner = spinner(Duration::from_secs(3600), 1);

        assert_eq!(spinner.get(), "b");
        assert_eq!(spinner.get(), "b");
    }

    #[test]
    fn get_advances_and_wraps_when_interval_elapses() {
        let spinner = spinner(Duration::ZERO, 2);

        assert_eq!(spinner.get(), "a");
        assert_eq!(spinner.get(), "b");
    }

    #[test]
    fn default_cycles_multiple_states() {
        let mut spinner = Spinner::default();
        spinner.interval = Duration::ZERO;

        let first = spinner.get();
        let second = spinner.get();

        assert_ne!(first, second);
    }
}
