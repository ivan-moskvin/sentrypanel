//! Sentry: the Events API, asked for one aggregate per metric.

mod client;
mod error;
mod plan;
mod query;
mod response;

pub use client::Client;
pub use error::FetchError;
pub use plan::{Plan, Source, plan};

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
                    active: slot.active.as_ref().map(
                        |source| matches!(value(answers, source), Ok(Some(count)) if count > 0.0),
                    ),
                })
                .collect()
        })
        .collect()
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

    #[test]
    fn reads_every_metric_and_its_marker() {
        let server = TestServer::routing(vec![
            (
                "query=active".into(),
                200,
                r#"{"data":[{"count()":3}]}"#.into(),
            ),
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
        assert_eq!(server.requests().len(), 3);
    }

    #[test]
    fn takes_a_count_of_zero_for_an_inactive_marker() {
        let server = TestServer::routing(vec![
            (
                "query=active".into(),
                200,
                r#"{"data":[{"count()":0}]}"#.into(),
            ),
            ("".into(), 200, r#"{"data":[]}"#.into()),
        ]);
        let (client, plan) = setup(&server);

        let readings = collect(&client, &plan).unwrap();
        assert_eq!(readings[0][0].value, Ok(None));
        assert_eq!(readings[0][0].active, Some(false));
    }

    #[test]
    fn stops_at_the_first_failure_of_the_whole_panel() {
        let server = TestServer::answering(401, "");
        let (client, plan) = setup(&server);

        assert_eq!(collect(&client, &plan), Err(FetchError::Auth(401)));
        assert_eq!(server.requests().len(), 1);
    }
}
