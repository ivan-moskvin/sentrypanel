//! Which requests a refresh makes. Metrics that share a dataset, a query, a
//! period and a project share a request: the API takes several fields at once.

use crate::config::{Metric, Section};
use crate::sentry::query::{MAX_FIELDS, Query};

/// Where the numbers of one metric are found in the answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub value: Source,
    pub active: Option<Source>,
}

/// One field of the answer to one of the queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub query: usize,
    pub field: String,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub queries: Vec<Query>,
    pub slots: Vec<Vec<Slot>>,
}

const COUNT: &str = "count()";

pub fn plan(sections: &[Section]) -> Plan {
    let mut plan = Plan::default();
    for section in sections {
        let slots = section
            .metrics
            .iter()
            .map(|metric| slot(&mut plan.queries, metric))
            .collect();
        plan.slots.push(slots);
    }
    plan
}

fn slot(queries: &mut Vec<Query>, metric: &Metric) -> Slot {
    let value = place(queries, scope(metric, &metric.query), &metric.field);
    let active = metric
        .active_when
        .as_ref()
        .map(|condition| place(queries, scope(metric, condition), COUNT));
    Slot { value, active }
}

fn scope(metric: &Metric, query: &str) -> Query {
    Query {
        dataset: metric.dataset.clone(),
        query: query.to_string(),
        period: metric.period.clone(),
        project: metric.project.clone(),
        fields: Vec::new(),
    }
}

fn place(queries: &mut Vec<Query>, wanted: Query, field: &str) -> Source {
    let source = |query: usize| Source {
        query,
        field: field.to_string(),
    };

    if let Some(index) = queries
        .iter()
        .position(|known| known.same_scope(&wanted) && known.fields.iter().any(|f| f == field))
    {
        return source(index);
    }
    if let Some(index) = queries
        .iter()
        .position(|known| known.same_scope(&wanted) && known.fields.len() < MAX_FIELDS)
    {
        queries[index].fields.push(field.to_string());
        return source(index);
    }

    let mut query = wanted;
    query.fields.push(field.to_string());
    queries.push(query);
    source(queries.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn sections(metrics: &str) -> Vec<Section> {
        let text = format!(
            "[sentry]\nbase_url = \"https://sentry.io\"\norg = \"acme\"\n[[section]]\ntitle = \"A\"\n{metrics}"
        );
        Config::parse(&text).unwrap().sections
    }

    fn metric(label: &str, query: &str, field: &str) -> String {
        format!(
            "[[section.metric]]\nlabel = \"{label}\"\ndataset = \"logs\"\nquery = \"{query}\"\nfield = \"{field}\"\nperiod = \"5m\"\n"
        )
    }

    #[test]
    fn shares_a_request_between_fields_of_one_scope() {
        let plan = plan(&sections(&format!(
            "{}{}{}",
            metric("A", "host:a", "avg(tags[x,number])"),
            metric("B", "host:a", "max(tags[x,number])"),
            metric("C", "host:b", "avg(tags[x,number])"),
        )));

        assert_eq!(plan.queries.len(), 2);
        assert_eq!(
            plan.queries[0].fields,
            ["avg(tags[x,number])", "max(tags[x,number])"]
        );
        assert_eq!(plan.slots[0][1].value.query, 0);
        assert_eq!(plan.slots[0][2].value.query, 1);
    }

    #[test]
    fn asks_for_a_field_once() {
        let plan = plan(&sections(&format!(
            "{}{}",
            metric("A", "host:a", "count()"),
            metric("B", "host:a", "count()"),
        )));
        assert_eq!(plan.queries.len(), 1);
        assert_eq!(plan.queries[0].fields, ["count()"]);
    }

    #[test]
    fn counts_the_condition_of_a_marker_in_its_own_scope() {
        let plan = plan(&sections(&format!(
            "{}active_when = \"active:a\"\n",
            metric("A", "host:a", "avg(x)")
        )));
        let active = plan.slots[0][0].active.clone().unwrap();

        assert_eq!(plan.queries.len(), 2);
        assert_eq!(plan.queries[active.query].query, "active:a");
        assert_eq!(active.field, "count()");
    }

    #[test]
    fn splits_a_scope_with_too_many_fields() {
        let mut queries = Vec::new();
        let wanted = Query {
            dataset: "spans".into(),
            query: String::new(),
            period: "1h".into(),
            project: None,
            fields: Vec::new(),
        };
        for index in 0..=MAX_FIELDS {
            place(
                &mut queries,
                wanted.clone(),
                &format!("p{index}(span.duration)"),
            );
        }

        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].fields.len(), MAX_FIELDS);
        assert_eq!(queries[1].fields.len(), 1);
    }
}
