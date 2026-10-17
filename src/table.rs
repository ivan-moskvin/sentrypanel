//! The panel as text, for `once`: the quickest way to check a config.

use crate::view::PanelView;

pub fn render(view: &PanelView) -> String {
    let mut out = String::new();
    for section in &view.sections {
        out.push_str(&section.title);
        out.push('\n');
        for row in &section.rows {
            let marker = if row.active { "*" } else { " " };
            let line = format!(
                "{marker} {:<6}  {:>8}  {:<7}  {}",
                row.label,
                row.value,
                row.status.word(),
                row.error.as_deref().unwrap_or("")
            );
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    if let Some(time) = &view.corner.time {
        out.push_str(&format!("fetched at {time}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric::Status;
    use crate::view::{Corner, RowView, SectionView};

    #[test]
    fn prints_a_line_per_metric() {
        let view = PanelView {
            sections: vec![SectionView {
                title: "SERVERS".into(),
                rows: vec![
                    RowView {
                        label: "WEB".into(),
                        value: "31ms".into(),
                        status: Status::Ok,
                        active: true,
                        error: None,
                    },
                    RowView {
                        label: "P95".into(),
                        value: "ERR".into(),
                        status: Status::Error,
                        active: false,
                        error: Some("HTTP 400: bad field".into()),
                    },
                ],
            }],
            corner: Corner {
                alert: None,
                time: Some("09:05".into()),
            },
            trusted: true,
        };

        assert_eq!(
            render(&view),
            "SERVERS\n\
             * WEB         31ms  ok\n\
             \x20 P95          ERR  error    HTTP 400: bad field\n\
             fetched at 09:05\n"
        );
    }
}
