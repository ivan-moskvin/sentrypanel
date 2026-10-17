//! Sentry: the Events API, asked for one aggregate per metric.

mod client;
mod error;
mod plan;
mod query;
mod response;

pub use client::Client;
pub use error::FetchError;
pub use plan::{Marker, Plan, Source, plan};

use response::Row;

/// What one metric read on the last refresh.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// `Ok(None)` is no data; `Err` is a request Sentry refused.
    pub value: Result<Option<f64>, String>,
    /// Whether the condition of the marker held, when the metric has one.
    pub active: Option<bool>,
}

/// The numbers of every metric, section by section. A failure that concerns
/// the whole panel ends the refresh at once: the rest would fail the same way,
/// each after its own timeout.
pub fn collect(client: &Client, plan: &Plan) -> Result<Vec<Vec<Reading>>, FetchError> {
    let mut answers = Vec::with_capacity(plan.queries.len());
    for query in &plan.queries {
        match client.fetch(query) {
            Ok(row) => answers.push(Ok(row)),
            Err(err) if err.is_global() => return Err(err),
            Err(err) => answers.push(Err(err.to_string())),
        }
    }
    Ok(read(plan, &answers))
}

fn read(plan: &Plan, answers: &[Result<Row, String>]) -> Vec<Vec<Reading>> {
    plan.slots
        .iter()
        .map(|slots| {
            slots
                .iter()
                .map(|slot| Reading {
                    value: value(answers, &slot.value),
                    active: slot.active.as_ref().map(|marker| active(answers, marker)),
                })
                .collect()
        })
        .collect()
}

/// A failed request or an empty window leaves the marker off: a dot is a
/// claim, and nothing backs it then.
fn active(answers: &[Result<Row, String>], marker: &Marker) -> bool {
    let newest = |index: usize| {
        let row = answers[index].as_ref().ok()?;
        response::moment(row, plan::PRECISE, plan::TIMESTAMP)
    };
    match (newest(marker.newest), newest(marker.newest_matching)) {
        (Some(newest), Some(matching)) => matching >= newest,
        _ => false,
    }
}

fn value(answers: &[Result<Row, String>], source: &Source) -> Result<Option<f64>, String> {
    let row = answers[source.query].as_ref().map_err(Clone::clone)?;
    response::number(row, &source.field)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::testserver::TestServer;
    use crate::token::Token;

    const CONFIG: &str = r#"
[sentry]
base_url = "http://placeholder"
org = "acme"

[[section]]
title = "HOSTS"

[[section.metric]]
label = "WEB"
dataset = "logs"
query = "host:web"
field = "avg(tags[ping_ms,number])"
active_when = "active:web"

[[section.metric]]
label = "BAD"
dataset = "spans"
field = "nonsense()"
"#;

    fn setup(server: &TestServer) -> (Client, Plan) {
        let mut config = Config::parse(CONFIG).unwrap();
        config.sentry.base_url = server.url();
        let client = Client::new(&config.sentry, Token::new("t"));
        (client, plan(&config.sections))
    }

    fn newest(nanos: u64) -> String {
        format!(
            r#"{{"data":[{{"timestamp":"2026-01-01T10:00:00+00:00","timestamp_precise":{nanos}}}]}}"#
        )
    }

    #[test]
    fn reads_every_metric_and_its_marker() {
        let server = TestServer::routing(vec![
            ("active%3Aweb".into(), 200, newest(2_000)),
            ("sort=-timestamp".into(), 200, newest(2_000)),
            (
                "dataset=logs".into(),
                200,
                r#"{"data":[{"avg(tags[ping_ms,number])":31.5}]}"#.into(),
            ),
            (
                "dataset=spans".into(),
                400,
                r#"{"detail":"Unknown function"}"#.into(),
            ),
        ]);
        let (client, plan) = setup(&server);

        let readings = collect(&client, &plan).unwrap();
        assert_eq!(readings[0][0].value, Ok(Some(31.5)));
        assert_eq!(readings[0][0].active, Some(true));
        assert!(
            readings[0][1]
                .value
                .as_ref()
                .unwrap_err()
                .contains("Unknown function")
        );
        assert_eq!(readings[0][1].active, None);
        assert_eq!(server.requests().len(), 4);
    }

    #[test]
    fn leaves_the_marker_off_when_nothing_matches() {
        let server = TestServer::routing(vec![
            ("active%3Aweb".into(), 200, r#"{"data":[]}"#.into()),
            ("sort=-timestamp".into(), 200, newest(2_000)),
            ("".into(), 200, r#"{"data":[]}"#.into()),
        ]);
        let (client, plan) = setup(&server);

        let readings = collect(&client, &plan).unwrap();
        assert_eq!(readings[0][0].value, Ok(None));
        assert_eq!(readings[0][0].active, Some(false));
    }

    /// Both values are in the window after a switch; only the newest counts.
    #[test]
    fn lights_only_the_marker_of_the_newest_record() {
        let config = r#"
[sentry]
base_url = "http://placeholder"
org = "acme"

[[section]]
title = "LINKS"

[[section.metric]]
label = "MAIN"
dataset = "logs"
query = "check:link"
field = "avg(tags[main_ms,number])"
active_when = "active:main"

[[section.metric]]
label = "BACKUP"
dataset = "logs"
query = "check:link"
field = "avg(tags[backup_ms,number])"
active_when = "active:backup"
"#;
        let server = TestServer::routing(vec![
            ("active%3Amain".into(), 200, newest(1_000)),
            ("active%3Abackup".into(), 200, newest(2_000)),
            ("sort=-timestamp".into(), 200, newest(2_000)),
            (
                "".into(),
                200,
                r#"{"data":[{"avg(tags[main_ms,number])":30,"avg(tags[backup_ms,number])":40}]}"#
                    .into(),
            ),
        ]);
        let mut config = Config::parse(config).unwrap();
        config.sentry.base_url = server.url();
        let client = Client::new(&config.sentry, Token::new("t"));

        let readings = collect(&client, &plan(&config.sections)).unwrap();
        assert_eq!(readings[0][0].active, Some(false), "main was switched off");
        assert_eq!(readings[0][1].active, Some(true));
    }

    #[test]
    fn stops_at_the_first_failure_of_the_whole_panel() {
        let server = TestServer::answering(401, "");
        let (client, plan) = setup(&server);

        assert_eq!(collect(&client, &plan), Err(FetchError::Auth(401)));
        assert_eq!(server.requests().len(), 1);
    }
}
