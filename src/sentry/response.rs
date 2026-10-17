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
    fn finds_the_detail_of_an_error() {
        assert_eq!(
            detail(r#"{"detail": "Invalid query. Unknown function"}"#),
            "Invalid query. Unknown function"
        );
        assert_eq!(detail(r#"{"detail": {"code": 1}}"#), r#"{"code":1}"#);
        assert_eq!(detail(" Bad Gateway \n"), "Bad Gateway");
    }
}
