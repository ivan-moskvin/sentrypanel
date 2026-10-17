//! Made-up numbers for `preview --demo`: enough of each color to judge the
//! layout without a token.

use crate::config::{Metric, Section, Worse};
use crate::sentry::Reading;

pub fn readings(sections: &[Section]) -> Vec<Vec<Reading>> {
    let mut index = 0;
    sections
        .iter()
        .map(|section| {
            let mut marked = false;
            section
                .metrics
                .iter()
                .map(|metric| {
                    let reading = Reading {
                        value: Ok(value(metric, index)),
                        active: metric
                            .active_when
                            .as_ref()
                            .map(|_| !std::mem::replace(&mut marked, true)),
                    };
                    index += 1;
                    reading
                })
                .collect()
        })
        .collect()
}

fn value(metric: &Metric, index: usize) -> Option<f64> {
    let shown = match index % 9 {
        2 => warning(metric),
        4 => return None,
        5 => critical(metric),
        _ => fine(metric),
    };
    Some(shown / metric.scale)
}

fn fine(metric: &Metric) -> f64 {
    match (metric.warn.or(metric.crit), metric.worse) {
        (Some(threshold), Worse::Higher) => threshold * 0.4,
        (Some(threshold), Worse::Lower) => threshold * 2.5,
        (None, _) => 42.0,
    }
}

fn warning(metric: &Metric) -> f64 {
    match (metric.warn, metric.crit) {
        (Some(warn), Some(crit)) => (warn + crit) / 2.0,
        (Some(warn), None) => warn,
        _ => fine(metric),
    }
}

fn critical(metric: &Metric) -> f64 {
    match (metric.crit, metric.worse) {
        (Some(crit), Worse::Higher) => crit * 1.3,
        (Some(crit), Worse::Lower) => crit * 0.5,
        (None, _) => warning(metric),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::metric::{Status, evaluate};

    #[test]
    fn shows_every_color_on_the_example() {
        let config = Config::parse(include_str!("../config.example.toml")).unwrap();
        let readings = readings(&config.sections);

        let statuses: Vec<Status> = config
            .metrics()
            .zip(readings.iter().flatten())
            .map(|(metric, reading)| evaluate(metric, *reading.value.as_ref().unwrap()))
            .collect();
        for wanted in [Status::Ok, Status::Warn, Status::Crit, Status::Unknown] {
            assert!(statuses.contains(&wanted), "{wanted:?} in {statuses:?}");
        }
    }

    #[test]
    fn marks_one_metric_per_section() {
        let config = Config::parse(include_str!("../config.example.toml")).unwrap();
        let readings = readings(&config.sections);
        let links: Vec<Option<bool>> = readings[1].iter().map(|reading| reading.active).collect();
        assert_eq!(links, [Some(true), Some(false)]);
        assert_eq!(readings[0][0].active, None);
    }

    #[test]
    fn falls_back_when_a_metric_has_no_thresholds() {
        let mut metric = Config::parse(include_str!("../config.example.toml"))
            .unwrap()
            .sections[0]
            .metrics[0]
            .clone();
        metric.warn = None;
        metric.crit = None;
        assert_eq!(value(&metric, 0), Some(42.0));
        assert_eq!(value(&metric, 2), Some(42.0));
        assert_eq!(value(&metric, 5), Some(42.0));
    }
}
