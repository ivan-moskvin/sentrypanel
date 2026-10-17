//! Drawing the panel. What the device does with the picture is the business of
//! divoomkit; what goes on the picture is ours.

pub mod layout;

use divoomkit::font::text_width;
use divoomkit::{Canvas, Frame, Rgb, Size};

use crate::metric::Status;
use crate::view::{Corner, PanelView, RowView};
use layout::{GLYPH_HEIGHT, SIZE};

const TITLE: Rgb = Rgb(0xa7, 0x8b, 0xfa);
const LABEL: Rgb = Rgb::GREY;
const DIM: Rgb = Rgb(0x6c, 0x6c, 0x6c);
const RULE: Rgb = Rgb::TRACK;
const MARKER: Rgb = Rgb::CYAN;

const MARGIN: i32 = 1;
const LABEL_X: i32 = 6;
const MARKER_SIZE: i32 = 3;
const SPACE: i32 = 4;

/// Shown instead of a value too wide for its row.
pub const OFF_SCALE: &str = ">>";

pub fn draw(view: &PanelView) -> Result<Frame, String> {
    let sizes: Vec<usize> = view
        .sections
        .iter()
        .map(|section| section.rows.len())
        .collect();
    let layout = layout::plan(&sizes)?;
    let mut canvas = Canvas::new(Size::Px128);

    for (index, (section, place)) in view.sections.iter().zip(&layout.sections).enumerate() {
        let corner = (index == 0).then_some(&view.corner);
        title(&mut canvas, place.title_y, &section.title, corner);
        for (row_view, y) in section.rows.iter().zip(&place.row_ys) {
            row(&mut canvas, *y, row_view, view.trusted);
        }
    }

    canvas
        .finish()
        .map_err(|err| format!("could not draw the panel: {err}"))
}

/// The title, a rule to the right of it, and — on the first one — the corner.
fn title(canvas: &mut Canvas, y: i32, text: &str, corner: Option<&Corner>) {
    canvas.text(MARGIN, y, 1, TITLE, text);

    let rule_from = MARGIN + width(text) + SPACE;
    let rule_to = match corner {
        Some(corner) => draw_corner(canvas, y, corner, rule_from) - SPACE,
        None => SIZE - MARGIN,
    };
    if rule_to > rule_from {
        canvas.rect(
            (
                rule_from,
                y + GLYPH_HEIGHT / 2,
                (rule_to - rule_from) as u32,
                1,
            ),
            RULE,
        );
    }
}

/// Draws the corner right-aligned and returns where it starts. The time gives
/// way to the alert when both do not fit beside the title.
fn draw_corner(canvas: &mut Canvas, y: i32, corner: &Corner, room_from: i32) -> i32 {
    let parts = corner_parts(corner, SIZE - MARGIN - room_from);

    let mut x = SIZE - MARGIN;
    for (text, color) in parts.iter().rev() {
        x -= width(text);
        canvas.text(x, y, 1, *color, text);
        x -= SPACE;
    }
    x + SPACE
}

fn corner_parts(corner: &Corner, room: i32) -> Vec<(String, Rgb)> {
    let mut parts = Vec::new();
    if let Some(alert) = &corner.alert {
        parts.push((alert.clone(), Rgb::RED));
    }
    if let Some(time) = &corner.time {
        let taken: i32 = parts.iter().map(|(text, _)| width(text) + SPACE).sum();
        if taken + width(time) <= room {
            parts.push((time.clone(), DIM));
        }
    }
    parts
}

fn row(canvas: &mut Canvas, y: i32, view: &RowView, trusted: bool) {
    if view.active {
        let color = if trusted { MARKER } else { DIM };
        let offset = (GLYPH_HEIGHT - MARKER_SIZE) / 2;
        canvas.rect(
            (MARGIN, y + offset, MARKER_SIZE as u32, MARKER_SIZE as u32),
            color,
        );
    }

    // Dimmed with the values: from across the room a whole panel gone grey
    // is what tells "not live", not the color of one number.
    let label = if trusted { LABEL } else { DIM };
    canvas.text(LABEL_X, y, 1, label, &view.label);

    let room = SIZE - MARGIN - (LABEL_X + width(&view.label) + SPACE);
    let value = fitted(&view.value, room);
    canvas.text(
        SIZE - MARGIN - width(value),
        y,
        1,
        color(view.status),
        value,
    );
}

/// The most digits a value is spelled with: `-9999`, `999.9`, `-999K` — after
/// that `ms` turns into `s` and a count into `K` or `M`.
const VALUE_DIGITS: usize = 5;

/// The widest value a metric with this unit is expected to show. `--` and
/// `ERR` are narrower.
pub fn widest_value(unit: &str) -> String {
    format!("{}{unit}", "9".repeat(VALUE_DIGITS))
}

/// How wide a label may be: what a row leaves once the marker, the gap and the
/// widest value are taken.
pub fn label_room(unit: &str) -> i32 {
    SIZE - MARGIN - LABEL_X - SPACE - width(&widest_value(unit))
}

pub fn text_px(text: &str) -> i32 {
    width(text)
}

/// How many characters fit into so many pixels.
pub fn chars_in(px: i32) -> i32 {
    let one = width("W");
    (px + 1) / (one + 1)
}

/// A value too wide for what the label leaves turns into `>>` rather than run
/// over the label: a number cut short would read as another number.
pub fn fitted(value: &str, room: i32) -> &str {
    if width(value) <= room {
        value
    } else {
        OFF_SCALE
    }
}

pub fn color(status: Status) -> Rgb {
    match status {
        Status::Ok => Rgb::GREEN,
        Status::Warn | Status::Error => Rgb::ORANGE,
        Status::Crit => Rgb::RED,
        Status::Unknown => DIM,
    }
}

fn width(text: &str) -> i32 {
    text_width(text, 1) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::SectionView;

    fn row_view(label: &str, value: &str, status: Status, active: bool) -> RowView {
        RowView {
            label: label.into(),
            value: value.into(),
            status,
            active,
            error: None,
        }
    }

    fn view(corner: Corner, trusted: bool) -> PanelView {
        let section = |title: &str| SectionView {
            title: title.into(),
            rows: vec![
                row_view("WEB", "31MS", Status::Ok, true),
                row_view("DB", "--", Status::Crit, false),
                row_view("DISK", "9.5%", Status::Warn, false),
                row_view("ERRORS", "ERR", Status::Error, false),
            ],
        };
        PanelView {
            sections: vec![section("SERVERS"), section("LINKS"), section("MY-APP")],
            corner,
            trusted,
        }
    }

    #[test]
    fn draws_a_frame_of_the_size_the_device_takes() {
        let frame = draw(&view(Corner::default(), true)).unwrap();
        assert_eq!(&frame.bytes()[..6], b"GIF89a");
        assert_eq!(&frame.bytes()[6..10], &[128, 0, 128, 0]);
    }

    #[test]
    fn looks_different_when_the_numbers_are_not_live() {
        let live = draw(&view(Corner::default(), true)).unwrap();
        let stale = draw(&view(
            Corner {
                alert: Some("AUTH".into()),
                time: Some("09:05".into()),
            },
            false,
        ))
        .unwrap();
        assert_ne!(live.hash(), stale.hash());
    }

    #[test]
    fn refuses_more_rows_than_fit() {
        let mut too_many = view(Corner::default(), true);
        too_many.sections[0].rows = vec![row_view("X", "1", Status::Ok, false); 20];
        assert!(draw(&too_many).is_err());
    }

    #[test]
    fn the_longest_label_leaves_room_for_the_widest_value() {
        for unit in ["", "ms", "%", "abc"] {
            let label = "W".repeat(chars_in(label_room(unit)) as usize);
            assert!(width(&label) <= label_room(unit), "{unit}");
            assert!(width(&format!("{label}W")) > label_room(unit), "{unit}");

            let room = SIZE - MARGIN - (LABEL_X + width(&label) + SPACE);
            for value in [
                widest_value(unit),
                "-9999".to_string(),
                "ERR".into(),
                "--".into(),
            ] {
                assert_eq!(fitted(&value, room), value, "{value} does not fit");
            }
        }
    }

    #[test]
    fn a_value_too_wide_gives_way_instead_of_running_over_the_label() {
        let label = "FRONT LCP";
        let room = SIZE - MARGIN - (LABEL_X + width(label) + SPACE);
        assert_eq!(fitted("123456789ms", room), OFF_SCALE);
        assert!(width(OFF_SCALE) <= room);

        let mut panel = view(Corner::default(), true);
        panel.sections[0].rows[0] = row_view(label, "123456789ms", Status::Ok, true);
        assert!(draw(&panel).is_ok());
    }

    #[test]
    fn draws_a_space_inside_a_label() {
        assert!(divoomkit::font::covers("API P95"));
        assert_eq!(width("API P95"), 7 * 6 - 1);
    }

    #[test]
    fn the_longest_title_leaves_room_for_the_alert() {
        let room = |title: &str| SIZE - MARGIN - (MARGIN + width(title) + SPACE);
        let corner = Corner {
            alert: Some("HTTP 503".into()),
            time: Some("23:59".into()),
        };

        let longest = corner_parts(&corner, room(&"W".repeat(crate::config::MAX_TITLE)));
        assert_eq!(longest.len(), 1, "the time gives way");
        assert_eq!(longest[0].0, "HTTP 503");

        let usual = corner_parts(&corner, room("SERVERS"));
        assert_eq!(usual.len(), 2, "both the alert and the time fit");
    }

    #[test]
    fn the_marker_stays_left_of_the_label() {
        const { assert!(MARGIN + MARKER_SIZE < LABEL_X) };
    }

    #[test]
    fn every_status_has_its_own_color_but_error_and_warn() {
        assert_ne!(color(Status::Ok), color(Status::Warn));
        assert_ne!(color(Status::Warn), color(Status::Crit));
        assert_ne!(color(Status::Crit), color(Status::Unknown));
        assert_eq!(color(Status::Unknown), DIM);
    }
}
