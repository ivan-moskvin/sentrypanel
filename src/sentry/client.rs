//! The HTTP side: one GET per query, with the token in the header.

use std::time::Duration;

use crate::config::Sentry;
use crate::sentry::error::FetchError;
use crate::sentry::query::Query;
use crate::sentry::response::{self, Row};
use crate::token::Token;

/// A refresh waits for every request in turn; a hanging one must not stall the
/// panel past its next refresh.
const TIMEOUT: Duration = Duration::from_secs(15);

pub struct Client {
    base_url: String,
    org: String,
    token: Token,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(sentry: &Sentry, token: Token) -> Client {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .user_agent(format!("sentrypanel/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Client {
            base_url: sentry.base_url.clone(),
            org: sentry.org.clone(),
            token,
            agent,
        }
    }

    pub fn fetch(&self, query: &Query) -> Result<Row, FetchError> {
        let url = query.url(&self.base_url, &self.org);
        let mut response = self
            .agent
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token.expose()))
            .call()
            .map_err(|err| FetchError::Offline(err.to_string()))?;

        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|err| FetchError::Offline(err.to_string()))?;
        answer(status, &body)
    }
}

fn answer(status: u16, body: &str) -> Result<Row, FetchError> {
    if status != 200 {
        return Err(FetchError::from_status(status, response::detail(body)));
    }
    response::first_row(body).map_err(FetchError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testserver::TestServer;

    fn sentry(base_url: String) -> Sentry {
        Sentry {
            base_url,
            org: "acme".into(),
            token_file: None,
        }
    }

    fn query() -> Query {
        Query {
            dataset: "spans".into(),
            query: "span.op:http.server".into(),
            period: "1h".into(),
            project: Some("my-app".into()),
            fields: vec!["p95(span.duration)".into()],
        }
    }

    #[test]
    fn asks_with_the_token_and_reads_the_row() {
        let server =
            TestServer::answering(200, r#"{"data":[{"p95(span.duration)":267.4}],"meta":{}}"#);
        let client = Client::new(&sentry(server.url()), Token::new("sntrys_test"));

        let row = client.fetch(&query()).unwrap();
        assert_eq!(
            response::number(&row, "p95(span.duration)"),
            Ok(Some(267.4))
        );

        let seen = server.requests();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].authorization, "Bearer sntrys_test");
        assert!(
            seen[0]
                .url
                .starts_with("/api/0/organizations/acme/events/?dataset=spans&")
        );
        assert!(seen[0].url.contains("projectSlug=my-app"));
    }

    #[test]
    fn maps_a_refused_token_to_auth() {
        let server = TestServer::answering(401, r#"{"detail":"Invalid token"}"#);
        let client = Client::new(&sentry(server.url()), Token::new("x"));
        assert_eq!(client.fetch(&query()), Err(FetchError::Auth(401)));
    }

    #[test]
    fn maps_a_refused_query_to_its_detail() {
        let server = TestServer::answering(400, r#"{"detail":"Unknown function foo"}"#);
        let client = Client::new(&sentry(server.url()), Token::new("x"));
        assert_eq!(
            client.fetch(&query()),
            Err(FetchError::Rejected(
                "HTTP 400: Unknown function foo".into()
            ))
        );
    }

    #[test]
    fn maps_a_garbled_answer_to_malformed() {
        let server = TestServer::answering(200, "<html>");
        let client = Client::new(&sentry(server.url()), Token::new("x"));
        assert!(matches!(
            client.fetch(&query()),
            Err(FetchError::Malformed(_))
        ));
    }

    #[test]
    fn maps_nobody_listening_to_offline() {
        let client = Client::new(&sentry("http://127.0.0.1:1".into()), Token::new("x"));
        assert!(matches!(
            client.fetch(&query()),
            Err(FetchError::Offline(_))
        ));
    }

    #[test]
    fn keeps_the_token_out_of_an_offline_error() {
        let client = Client::new(
            &sentry("http://127.0.0.1:1".into()),
            Token::new("sntrys_secret"),
        );
        let err = client.fetch(&query()).unwrap_err().to_string();
        assert!(!err.contains("sntrys_secret"), "{err}");
    }
}
