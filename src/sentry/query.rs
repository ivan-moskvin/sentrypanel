//! One request to the Events API and the URL that carries it.

/// The API refuses more fields than this in one request.
pub const MAX_FIELDS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub dataset: String,
    pub query: String,
    pub period: String,
    pub project: Option<String>,
    pub fields: Vec<String>,
    /// The newest record of the scope instead of an aggregate over it.
    pub latest: bool,
}

impl Query {
    pub fn same_scope(&self, other: &Query) -> bool {
        self.dataset == other.dataset
            && self.query == other.query
            && self.period == other.period
            && self.project == other.project
            && self.latest == other.latest
    }

    pub fn url(&self, base_url: &str, org: &str) -> String {
        let mut params: Vec<(&str, &str)> = vec![("dataset", &self.dataset)];
        for field in &self.fields {
            params.push(("field", field));
        }
        if !self.query.is_empty() {
            params.push(("query", &self.query));
        }
        params.push(("statsPeriod", &self.period));
        params.push(project_param(self.project.as_deref()));
        if self.latest {
            params.push(("sort", "-timestamp"));
        }
        params.push(("per_page", "1"));

        let query: Vec<String> = params
            .iter()
            .map(|(key, value)| format!("{key}={}", encode(value)))
            .collect();
        format!(
            "{base_url}/api/0/organizations/{}/events/?{}",
            encode(org),
            query.join("&")
        )
    }
}

/// Without a project the API falls back to the projects of the caller's teams,
/// which silently hides the rest of the organization; `-1` asks for every
/// project the token can see. A slug goes as `projectSlug`: older and
/// self-hosted Sentry versions take only numeric ids in `project`.
fn project_param(project: Option<&str>) -> (&'static str, &str) {
    match project {
        None => ("project", "-1"),
        Some(id) if id == "-1" || id.chars().all(|digit| digit.is_ascii_digit()) => ("project", id),
        Some(slug) => ("projectSlug", slug),
    }
}

/// Percent-encodes everything but the unreserved characters of RFC 3986. A
/// Sentry query is full of `:`, `[`, `,`, `"` and spaces, and each of them
/// means something else in a URL.
pub fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(project: Option<&str>) -> Query {
        Query {
            dataset: "logs".into(),
            query: "host:web-1 message:\"a b\"".into(),
            period: "5m".into(),
            project: project.map(String::from),
            fields: vec!["avg(tags[ping_ms,number])".into(), "count()".into()],
            latest: false,
        }
    }

    #[test]
    fn spells_the_events_request() {
        assert_eq!(
            query(None).url("https://de.sentry.io", "acme"),
            "https://de.sentry.io/api/0/organizations/acme/events/\
             ?dataset=logs\
             &field=avg%28tags%5Bping_ms%2Cnumber%5D%29\
             &field=count%28%29\
             &query=host%3Aweb-1%20message%3A%22a%20b%22\
             &statsPeriod=5m\
             &project=-1\
             &per_page=1"
        );
    }

    #[test]
    fn sends_an_id_as_project_and_a_slug_as_project_slug() {
        let by_id = query(Some("4501")).url("https://sentry.io", "acme");
        assert!(by_id.contains("&project=4501&"), "{by_id}");

        let by_slug = query(Some("my-app")).url("https://sentry.io", "acme");
        assert!(by_slug.contains("&projectSlug=my-app&"), "{by_slug}");
        assert!(!by_slug.contains("&project="), "{by_slug}");
    }

    #[test]
    fn asks_for_the_newest_record_first() {
        let mut latest = query(None);
        latest.fields = vec!["timestamp".into()];
        latest.latest = true;
        let url = latest.url("https://sentry.io", "acme");
        assert!(url.contains("&sort=-timestamp&per_page=1"), "{url}");
        assert!(
            !query(None)
                .url("https://sentry.io", "acme")
                .contains("sort=")
        );
        assert!(!latest.same_scope(&query(None)));
    }

    #[test]
    fn leaves_out_an_empty_query() {
        let mut empty = query(None);
        empty.query.clear();
        assert!(!empty.url("https://sentry.io", "acme").contains("query="));
    }

    #[test]
    fn escapes_the_organization_too() {
        let url = query(None).url("https://sentry.io", "a/b?c");
        assert!(url.contains("/organizations/a%2Fb%3Fc/events/"), "{url}");
    }

    #[test]
    fn encodes_everything_but_the_unreserved_characters() {
        assert_eq!(encode("aZ09-_.~"), "aZ09-_.~");
        assert_eq!(encode("a b&c=d+e#f"), "a%20b%26c%3Dd%2Be%23f");
        assert_eq!(encode("!tags[x]:\"y\""), "%21tags%5Bx%5D%3A%22y%22");
        assert_eq!(encode("я"), "%D1%8F");
    }

    #[test]
    fn tells_queries_of_one_scope() {
        let mut other = query(None);
        other.fields = vec!["max(x)".into()];
        assert!(query(None).same_scope(&other));

        other.period = "1h".into();
        assert!(!query(None).same_scope(&other));
    }
}
