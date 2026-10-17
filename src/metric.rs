//! What a number means: its color and how it is spelled on the panel.

use crate::config::{Metric, Missing, Worse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Crit,
    /// No data, or numbers that can no longer be trusted.
    Unknown,
    /// Sentry refused the request of this metric.
    Error,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Crit => "crit",
            Status::Unknown => "no data",
            Status::Error => "error",
        }
    }
}

pub fn evaluate(metric: &Metric, value: Option<f64>) -> Status {
    let Some(value) = value else {
        return match metric.missing {
            Missing::Grey => Status::Unknown,
            Missing::Warn => Status::Warn,
            Missing::Crit => Status::Crit,
        };
    };

    let value = value * metric.scale;
    let reached = |threshold: Option<f64>| {
        threshold.is_some_and(|threshold| match metric.worse {
            Worse::Higher => value >= threshold,
            Worse::Lower => value <= threshold,
        })
    };

    if reached(metric.crit) {
        Status::Crit
    } else if reached(metric.warn) {
        Status::Warn
    } else {
        Status::Ok
    }
}

pub const NO_VALUE: &str = "--";

/// The value with its unit, short enough for the right half of a row.
pub fn format(metric: &Metric, value: Option<f64>) -> String {
    let Some(value) = value else {
        return NO_VALUE.into();
    };
    let value = value * metric.scale;

    match metric.unit.as_str() {
        "ms" if value.abs() >= 10_000.0 => format!("{}s", number(value / 1000.0)),
        "" => count(value),
        unit => format!("{}{unit}", number(value)),
    }
}

/// One decimal while it says something, none once the number is large enough
/// for the decimal to be noise.
fn number(value: f64) -> String {
    if value.abs() < 10.0 && value.fract().abs() >= 0.05 {
        return format!("{value:.1}");
    }
    format!("{}", value.round() as i64)
}

fn count(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 1_000_000.0 {
        return format!("{}M", number(value / 1_000_000.0));
    }
    if magnitude >= 10_000.0 {
        return format!("{}K", number(value / 1000.0));
    }
    number(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::minimal;

    fn metric(warn: Option<f64>, crit: Option<f64>, worse: Worse) -> Metric {
        let mut metric = minimal().sections[0].metrics[0].clone();
        metric.warn = warn;
        metric.crit = crit;
        metric.worse = worse;
        metric
    }

    #[test]
    fn colors_by_thresholds_where_higher_is_worse() {
        let metric = metric(Some(80.0), Some(200.0), Worse::Higher);
        assert_eq!(evaluate(&metric, Some(31.0)), Status::Ok);
        assert_eq!(evaluate(&metric, Some(80.0)), Status::Warn);
        assert_eq!(evaluate(&metric, Some(199.9)), Status::Warn);
        assert_eq!(evaluate(&metric, Some(200.0)), Status::Crit);
    }

    #[test]
    fn colors_by_thresholds_where_lower_is_worse() {
        let metric = metric(Some(20.0), Some(10.0), Worse::Lower);
        assert_eq!(evaluate(&metric, Some(55.0)), Status::Ok);
        assert_eq!(evaluate(&metric, Some(20.0)), Status::Warn);
        assert_eq!(evaluate(&metric, Some(10.0)), Status::Crit);
        assert_eq!(evaluate(&metric, Some(0.0)), Status::Crit);
    }

    #[test]
    fn works_with_one_threshold_or_none() {
        assert_eq!(
            evaluate(&metric(None, None, Worse::Higher), Some(1e9)),
            Status::Ok
        );
        assert_eq!(
            evaluate(&metric(None, Some(5.0), Worse::Higher), Some(6.0)),
            Status::Crit
        );
        assert_eq!(
            evaluate(&metric(Some(5.0), None, Worse::Higher), Some(6.0)),
            Status::Warn
        );
    }

    #[test]
    fn compares_the_scaled_value() {
        let mut metric = metric(Some(1.0), Some(5.0), Worse::Higher);
        metric.scale = 100.0;
        assert_eq!(evaluate(&metric, Some(0.02)), Status::Warn);
        assert_eq!(evaluate(&metric, Some(0.06)), Status::Crit);
    }

    #[test]
    fn colors_missing_data_as_configured() {
        let mut metric = metric(Some(1.0), Some(2.0), Worse::Higher);
        assert_eq!(evaluate(&metric, None), Status::Unknown);
        metric.missing = Missing::Warn;
        assert_eq!(evaluate(&metric, None), Status::Warn);
        metric.missing = Missing::Crit;
        assert_eq!(evaluate(&metric, None), Status::Crit);
    }

    fn with_unit(unit: &str) -> Metric {
        let mut metric = minimal().sections[0].metrics[0].clone();
        metric.unit = unit.into();
        metric
    }

    #[test]
    fn spells_milliseconds() {
        let ms = with_unit("ms");
        assert_eq!(format(&ms, Some(31.38)), "31ms");
        assert_eq!(format(&ms, Some(3.44)), "3.4ms");
        assert_eq!(format(&ms, Some(3.0)), "3ms");
        assert_eq!(format(&ms, Some(9_999.0)), "9999ms");
        assert_eq!(format(&ms, Some(12_345.0)), "12s");
        assert_eq!(format(&ms, None), "--");
    }

    #[test]
    fn spells_percents_and_other_units() {
        assert_eq!(format(&with_unit("%"), Some(1.53)), "1.5%");
        assert_eq!(format(&with_unit("%"), Some(45.4)), "45%");
        assert_eq!(format(&with_unit("s"), Some(2.26)), "2.3s");
        assert_eq!(format(&with_unit("°"), Some(-3.0)), "-3°");

        let mut rate = with_unit("%");
        rate.scale = 100.0;
        assert_eq!(format(&rate, Some(0.0123)), "1.2%");
    }

    #[test]
    fn spells_counts() {
        let plain = with_unit("");
        assert_eq!(format(&plain, Some(6.0)), "6");
        assert_eq!(format(&plain, Some(9_999.0)), "9999");
        assert_eq!(format(&plain, Some(12_500.0)), "13K");
        assert_eq!(format(&plain, Some(2_400_000.0)), "2.4M");
    }

    #[test]
    fn has_a_word_for_every_status() {
        for status in [
            Status::Ok,
            Status::Warn,
            Status::Crit,
            Status::Unknown,
            Status::Error,
        ] {
            assert!(!status.word().is_empty());
        }
    }
}
