//! Reading the answer of the Events API: `{"data": [{...}], "meta": {...}}`.

use serde::Deserialize;
use serde_json::{Map, Value};

pub type Row = Map<String, Value>;

#[derive(Deserialize)]
struct Answer {
    data: Vec<Row>,
}

/// An aggregate without a group-by comes back as one row. No rows at all is no
/// data, not an error.
pub fn first_row(body: &str) -> Result<Row, String> {
    let answer: Answer = serde_json::from_str(body)
        .map_err(|err| format!("Sentry answered with something unexpected: {err}"))?;
    Ok(answer.data.into_iter().next().unwrap_or_default())
}

/// An aggregate over nothing is `null`; a field the row does not carry is the
/// same thing.
pub fn number(row: &Row, field: &str) -> Result<Option<f64>, String> {
    match row.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => Ok(number.as_f64()),
        Some(Value::String(text)) => text
            .parse::<f64>()
            .map(Some)
            .map_err(|_| format!("{field} is not a number: {text:?}")),
        Some(other) => Err(format!("{field} is not a number: {other}")),
    }
}

/// When a record was written, as precisely as the answer says.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Moment {
    Nanos(i128),
    Text(String),
}

/// The moment of the newest record in an answer, `None` when there was none.
/// Both timestamps come from the same API, so text compares as time.
pub fn moment(row: &Row, precise: &str, timestamp: &str) -> Option<Moment> {
    let nanos = match row.get(precise) {
        Some(Value::Number(number)) => number
            .as_i64()
            .map(i128::from)
            .or_else(|| number.as_u64().map(i128::from)),
        Some(Value::String(text)) => text.parse().ok(),
        _ => None,
    };
    if let Some(nanos) = nanos {
        return Some(Moment::Nanos(nanos));
    }
    match row.get(timestamp) {
        Some(Value::String(text)) if !text.is_empty() => Some(Moment::Text(text.clone())),
        Some(Value::Number(number)) => Some(Moment::Text(number.to_string())),
        _ => None,
    }
}

/// The `detail` Sentry puts into an error answer, or the body itself.
pub fn detail(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.get("detail").map(describe))
        .unwrap_or_else(|| body.trim().chars().take(200).collect())
}

fn describe(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{
        "data": [{"avg(tags[ping_ms,number])": 31.38, "count()": 188, "p95(span.duration)": null}],
        "meta": {"fields": {"avg(tags[ping_ms,number])": "number", "count()": "integer"}}
    }"#;

    #[test]
    fn reads_the_numbers_of_the_first_row() {
        let row = first_row(ANSWER).unwrap();
        assert_eq!(number(&row, "avg(tags[ping_ms,number])"), Ok(Some(31.38)));
        assert_eq!(number(&row, "count()"), Ok(Some(188.0)));
    }

    #[test]
    fn takes_null_and_absence_for_no_data() {
        let row = first_row(ANSWER).unwrap();
        assert_eq!(number(&row, "p95(span.duration)"), Ok(None));
        assert_eq!(number(&row, "never_asked()"), Ok(None));

        let empty = first_row(r#"{"data": [], "meta": {}}"#).unwrap();
        assert_eq!(number(&empty, "count()"), Ok(None));
    }

    #[test]
    fn reads_a_number_sent_as_a_string() {
        let row = first_row(r#"{"data": [{"x": "12.5", "y": "n/a", "z": [1]}]}"#).unwrap();
        assert_eq!(number(&row, "x"), Ok(Some(12.5)));
        assert!(number(&row, "y").is_err());
        assert!(number(&row, "z").is_err());
    }

    #[test]
    fn refuses_what_is_not_an_answer() {
        assert!(first_row("<html>").is_err());
        assert!(first_row(r#"{"meta": {}}"#).is_err());
    }

    #[test]
    fn reads_the_moment_of_a_record() {
        let precise = first_row(
            r#"{"data": [{"timestamp": "2026-01-01T10:00:00+00:00", "timestamp_precise": 1767261600123456789}]}"#,
        )
        .unwrap();
        assert_eq!(
            moment(&precise, "timestamp_precise", "timestamp"),
            Some(Moment::Nanos(1767261600123456789))
        );

        let coarse =
            first_row(r#"{"data": [{"timestamp": "2026-01-01T10:00:00+00:00"}]}"#).unwrap();
        assert_eq!(
            moment(&coarse, "timestamp_precise", "timestamp"),
            Some(Moment::Text("2026-01-01T10:00:00+00:00".into()))
        );

        let none = first_row(r#"{"data": []}"#).unwrap();
        assert_eq!(moment(&none, "timestamp_precise", "timestamp"), None);
    }

    #[test]
    fn finds_the_detail_of_an_error() {
        assert_eq!(
            detail(r#"{"detail": "Invalid query. Unknown function"}"#),
            "Invalid query. Unknown function"
        );
        assert_eq!(detail(r#"{"detail": {"code": 1}}"#), r#"{"code":1}"#);
        assert_eq!(detail(" Bad Gateway \n"), "Bad Gateway");
    }
}
