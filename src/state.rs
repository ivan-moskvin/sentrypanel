//! What the panel knows between refreshes.

use chrono::{DateTime, Local};

use crate::sentry::{FetchError, Reading};

#[derive(Debug, Default)]
pub struct State {
    /// The numbers of the last refresh that worked.
    pub readings: Option<Vec<Vec<Reading>>>,
    pub fetched_at: Option<DateTime<Local>>,
    /// Why the last refresh did not work. The old numbers stay, but the panel
    /// must not show them as if they were fresh.
    pub failure: Option<FetchError>,
}

impl State {
    pub fn record(&mut self, result: Result<Vec<Vec<Reading>>, FetchError>, now: DateTime<Local>) {
        match result {
            Ok(readings) => {
                self.readings = Some(readings);
                self.fetched_at = Some(now);
                self.failure = None;
            }
            Err(err) => self.failure = Some(err),
        }
    }

    pub fn reading(&self, section: usize, metric: usize) -> Option<&Reading> {
        self.readings.as_ref()?.get(section)?.get(metric)
    }

    pub fn trusted(&self) -> bool {
        self.failure.is_none() && self.readings.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(value: f64) -> Reading {
        Reading {
            value: Ok(Some(value)),
            active: None,
        }
    }

    #[test]
    fn keeps_the_last_numbers_through_a_failure() {
        let mut state = State::default();
        assert!(!state.trusted());

        let first = Local::now();
        state.record(Ok(vec![vec![reading(1.0)]]), first);
        assert!(state.trusted());

        state.record(Err(FetchError::Auth(401)), Local::now());
        assert!(!state.trusted());
        assert_eq!(state.reading(0, 0), Some(&reading(1.0)));
        assert_eq!(state.fetched_at, Some(first));

        state.record(Ok(vec![vec![reading(2.0)]]), Local::now());
        assert!(state.trusted());
        assert_eq!(state.reading(0, 0), Some(&reading(2.0)));
        assert_eq!(state.reading(0, 1), None);
        assert_eq!(state.reading(3, 0), None);
    }
}
