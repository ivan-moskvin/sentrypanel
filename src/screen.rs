//! `screen N`: moves the panel by editing the config in place, keeping the
//! comments and the order a human gave it.

pub fn parse(asked: &str) -> Result<u8, String> {
    let screens = divoomkit::SCREEN_COUNT;
    asked
        .parse()
        .ok()
        .filter(|number| (1..=screens).contains(number))
        .ok_or_else(|| format!("the screen is a number from 1 to {screens}, not {asked:?}"))
}

pub fn set(text: &str, screen: u8) -> Result<String, String> {
    let mut document: toml_edit::DocumentMut = text.parse().map_err(|err| format!("{err}"))?;
    let device = document
        .entry("device")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let table = device
        .as_table_like_mut()
        .ok_or("[device] in the config is not a table")?;
    table.insert("screen", toml_edit::value(i64::from(screen)));
    Ok(document.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_screen_number() {
        assert_eq!(parse("1"), Ok(1));
        assert_eq!(parse("5"), Ok(5));
        for bad in ["0", "6", "two", "-1", ""] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn changes_the_screen_and_keeps_the_rest() {
        let before =
            "# my panel\n[device]\nscreen = 1 # left\nport = 9000\n\n[sentry]\norg = \"acme\"\n";
        let after = set(before, 4).unwrap();

        assert!(after.contains("# my panel"), "{after}");
        assert!(after.contains("screen = 4"), "{after}");
        assert!(after.contains("port = 9000"), "{after}");
        assert!(after.contains("org = \"acme\""), "{after}");
    }

    #[test]
    fn adds_a_device_table_when_there_is_none() {
        let after = set("[sentry]\norg = \"acme\"\n", 2).unwrap();
        assert!(after.contains("[device]\nscreen = 2"), "{after}");
    }

    #[test]
    fn refuses_a_config_it_cannot_edit() {
        assert!(set("[device\n", 2).is_err());
        assert!(set("device = 3\n", 2).is_err());
    }
}
