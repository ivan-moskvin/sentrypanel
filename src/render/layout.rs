//! Where the lines go on 128 pixels. Every line is the 5×7 font at scale 1:
//! a dozen rows leave no room for anything larger.

pub const SIZE: i32 = 128;
pub const GLYPH_HEIGHT: i32 = 7;

const TOP: i32 = 1;
const TITLE_PITCH: i32 = 9;
const MIN_ROW_PITCH: i32 = 8;
const MAX_ROW_PITCH: i32 = 11;
const SECTION_GAP: i32 = 2;
/// Spare height goes between the sections, up to this much; beyond it the
/// panel would look like three separate panels.
const MAX_EXTRA_GAP: i32 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub sections: Vec<SectionPlace>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionPlace {
    pub title_y: i32,
    pub row_ys: Vec<i32>,
}

/// Rows are spread as far apart as the screen allows: the fewer there are, the
/// easier they are to tell apart from across the room.
pub fn plan(section_sizes: &[usize]) -> Result<Layout, String> {
    let sections = section_sizes.len() as i32;
    let rows: i32 = section_sizes.iter().sum::<usize>() as i32;

    let pitch = (MIN_ROW_PITCH..=MAX_ROW_PITCH)
        .rev()
        .find(|pitch| height(sections, rows, *pitch, SECTION_GAP) <= SIZE)
        .ok_or_else(|| too_many(sections, rows))?;

    let spare = SIZE - height(sections, rows, pitch, SECTION_GAP);
    let gap = SECTION_GAP + (spare / (sections - 1).max(1)).min(MAX_EXTRA_GAP);

    let mut y = TOP;
    let places = section_sizes
        .iter()
        .map(|size| {
            let title_y = y;
            y += TITLE_PITCH;
            let row_ys = (0..*size)
                .map(|_| {
                    let row_y = y;
                    y += pitch;
                    row_y
                })
                .collect();
            y += gap;
            SectionPlace { title_y, row_ys }
        })
        .collect();

    Ok(Layout { sections: places })
}

fn height(sections: i32, rows: i32, pitch: i32, gap: i32) -> i32 {
    TOP + sections * TITLE_PITCH + rows * pitch + (sections - 1).max(0) * gap
        - (pitch - GLYPH_HEIGHT)
}

fn too_many(sections: i32, rows: i32) -> String {
    let room = SIZE - height(sections, 0, MIN_ROW_PITCH, SECTION_GAP) - GLYPH_HEIGHT;
    let fit = (room / MIN_ROW_PITCH + 1).max(0);
    format!(
        "{rows} metrics in {sections} sections do not fit the screen: at most {fit} with {sections} sections"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bottom(layout: &Layout) -> i32 {
        let last = layout.sections.last().unwrap();
        last.row_ys.last().copied().unwrap_or(last.title_y) + GLYPH_HEIGHT
    }

    #[test]
    fn fits_a_dozen_rows_in_three_sections() {
        let layout = plan(&[4, 4, 4]).unwrap();
        assert!(bottom(&layout) <= SIZE);
        assert_eq!(layout.sections[0].title_y, TOP);
        assert_eq!(layout.sections[0].row_ys[0], TOP + TITLE_PITCH);
    }

    #[test]
    fn spreads_fewer_rows_wider() {
        let few = plan(&[2, 2]).unwrap();
        let many = plan(&[5, 6]).unwrap();
        let pitch = |layout: &Layout| layout.sections[0].row_ys[1] - layout.sections[0].row_ys[0];

        assert_eq!(pitch(&few), MAX_ROW_PITCH);
        assert!(pitch(&many) < pitch(&few));
        assert!(bottom(&few) <= SIZE);
    }

    #[test]
    fn never_lets_lines_touch_or_leave_the_screen() {
        for sections in 1..=5usize {
            for rows in sections..=16 {
                let mut sizes = vec![rows / sections; sections];
                sizes[0] += rows % sections;
                let Ok(layout) = plan(&sizes) else { continue };

                let mut lines: Vec<i32> = Vec::new();
                for place in &layout.sections {
                    lines.push(place.title_y);
                    lines.extend(&place.row_ys);
                }
                assert!(
                    lines
                        .windows(2)
                        .all(|pair| pair[1] - pair[0] > GLYPH_HEIGHT),
                    "{sizes:?}"
                );
                assert!(bottom(&layout) <= SIZE, "{sizes:?}");
            }
        }
    }

    #[test]
    fn says_how_many_rows_would_fit() {
        let err = plan(&[5, 5, 5]).unwrap_err();
        assert!(err.contains("15 metrics in 3 sections"), "{err}");

        let fit: usize = err
            .rsplit("at most ")
            .next()
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|number| number.parse().ok())
            .unwrap();
        assert!(plan(&[fit - 2, 1, 1]).is_ok(), "{fit} rows were promised");
        assert!(plan(&[fit - 1, 1, 1]).is_err());
    }
}
