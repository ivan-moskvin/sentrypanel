//! What goes on the panel, in words and statuses — shared by the picture and by
//! the table of `once`.

use crate::config::Section;
use crate::metric::{self, Status};
use crate::state::State;

#[derive(Debug, Clone, PartialEq)]
pub struct PanelView {
    pub sections: Vec<SectionView>,
    pub corner: Corner,
    pub trusted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionView {
    pub title: String,
    pub rows: Vec<RowView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowView {
    pub label: String,
    pub value: String,
    pub status: Status,
    pub active: bool,
    /// Why the value is missing, for the table; the panel has no room for it.
    pub error: Option<String>,
}

/// The top right corner: the time of the last refresh that worked and, when the
/// last one did not, the word for why.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Corner {
    pub alert: Option<String>,
    pub time: Option<String>,
}

pub const ERROR_VALUE: &str = "ERR";

pub fn build(sections: &[Section], state: &State) -> PanelView {
    let trusted = state.trusted();
    let sections = sections
        .iter()
        .enumerate()
        .map(|(index, section)| SectionView {
            title: section.title.clone(),
            rows: section
                .metrics
                .iter()
                .enumerate()
                .map(|(position, metric)| row(metric, state.reading(index, position), trusted))
                .collect(),
        })
        .collect();

    PanelView {
        sections,
        corner: Corner {
            alert: state.failure.as_ref().map(|err| err.short()),
            time: state.fetched_at.map(|at| at.format("%H:%M").to_string()),
        },
        trusted,
    }
}

fn row(
    metric: &crate::config::Metric,
    reading: Option<&crate::sentry::Reading>,
    trusted: bool,
) -> RowView {
    let (value, status, error) = match reading.map(|reading| &reading.value) {
        None => (metric::NO_VALUE.to_string(), Status::Unknown, None),
        Some(Err(err)) => (ERROR_VALUE.to_string(), Status::Error, Some(err.clone())),
        Some(Ok(value)) => (
            metric::format(metric, *value),
            metric::evaluate(metric, *value),
            None,
        ),
    };

    RowView {
        label: metric.label.clone(),
        value,
        // Old numbers in their old colors read as all clear; grey reads as
        // "this is not live".
        status: if trusted { status } else { Status::Unknown },
        active: reading.and_then(|reading| reading.active).unwrap_or(false),
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::sentry::{FetchError, Reading};
    use chrono::{Local, TimeZone};

    fn sections() -> Vec<Section> {
        Config::parse(include_str!("../config.example.toml"))
            .unwrap()
            .sections
    }

    fn reading(value: Option<f64>, active: Option<bool>) -> Reading {
        Reading {
            value: Ok(value),
            active,
        }
    }

    fn state() -> State {
        let mut readings: Vec<Vec<Reading>> = sections()
            .iter()
            .map(|section| {
                section
                    .metrics
                    .iter()
                    .map(|_| reading(Some(1.0), None))
                    .collect()
            })
            .collect();
        readings[0][0] = reading(Some(31.4), None);
        readings[0][1] = reading(None, None);
        readings[1][0] = reading(Some(500.0), Some(true));
        readings[2][0] = Reading {
            value: Err("Unknown function".into()),
            active: None,
        };

        let mut state = State::default();
        let at = Local.with_ymd_and_hms(2026, 10, 17, 9, 5, 0).unwrap();
        state.record(Ok(readings), at);
        state
    }

    #[test]
    fn spells_and_colors_every_row() {
        let view = build(&sections(), &state());
        let servers = &view.sections[0];

        assert_eq!(servers.title, "SERVERS");
        assert_eq!(servers.rows[0].value, "31ms");
        assert_eq!(servers.rows[0].status, Status::Ok);
        assert_eq!(servers.rows[1].value, "--");
        assert_eq!(
            servers.rows[1].status,
            Status::Crit,
            "a silent host is an alarm"
        );

        let link = &view.sections[1].rows[0];
        assert_eq!(link.status, Status::Crit);
        assert!(link.active);
        assert!(!view.sections[1].rows[1].active);

        let errors = &view.sections[2].rows[0];
        assert_eq!(errors.value, ERROR_VALUE);
        assert_eq!(errors.status, Status::Error);
        assert_eq!(errors.error.as_deref(), Some("Unknown function"));

        assert_eq!(view.corner.time.as_deref(), Some("09:05"));
        assert_eq!(view.corner.alert, None);
        assert!(view.trusted);
    }

    #[test]
    fn greys_everything_out_after_a_failure() {
        let mut state = state();
        state.record(Err(FetchError::Offline("timeout".into())), Local::now());
        let view = build(&sections(), &state);

        assert!(!view.trusted);
        assert_eq!(view.corner.alert.as_deref(), Some("OFFLINE"));
        assert_eq!(view.corner.time.as_deref(), Some("09:05"));
        for row in view.sections.iter().flat_map(|section| &section.rows) {
            assert_eq!(row.status, Status::Unknown, "{}", row.label);
        }
        assert_eq!(
            view.sections[0].rows[0].value, "31ms",
            "the last numbers stay"
        );
    }

    #[test]
    fn shows_nothing_before_the_first_refresh() {
        let view = build(&sections(), &State::default());
        assert_eq!(view.corner, Corner::default());
        assert!(view.sections[0].rows.iter().all(|row| row.value == "--"));
    }
}
