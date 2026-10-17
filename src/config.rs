//! The config file: where Sentry is, which device, and what goes on the screen.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::render::{self, layout};

const DEFAULT_REFRESH_SECS: u64 = 60;
/// Every refresh is one request per group of metrics; anything faster than this
/// spends the rate limit of the organization on a picture nobody looks at that
/// often.
const MIN_REFRESH_SECS: u64 = 10;
const DEFAULT_SCREEN: u8 = 1;
/// Not the port of claudestatus: both may drive screens of the same device.
const DEFAULT_PORT: u16 = 8478;

pub const MAX_TITLE: usize = 12;
pub const MAX_UNIT: usize = 3;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub sentry: Sentry,
    #[serde(default)]
    pub device: DeviceConfig,
    #[serde(default = "default_refresh")]
    pub refresh_secs: u64,
    #[serde(default, rename = "section")]
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sentry {
    pub base_url: String,
    pub org: String,
    #[serde(default)]
    pub token_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Numbered 1 to 5, the way the Divoom app labels the screens.
    #[serde(default = "default_screen")]
    pub screen: u8,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            ip: None,
            name: None,
            screen: DEFAULT_SCREEN,
            port: DEFAULT_PORT,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub title: String,
    #[serde(default, rename = "metric")]
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub label: String,
    pub dataset: String,
    #[serde(default)]
    pub query: String,
    pub field: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default = "default_period")]
    pub period: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub warn: Option<f64>,
    #[serde(default)]
    pub crit: Option<f64>,
    #[serde(default)]
    pub worse: Worse,
    #[serde(default)]
    pub missing: Missing,
    #[serde(default)]
    pub active_when: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Worse {
    #[default]
    Higher,
    Lower,
}

/// What a metric with no data means. For a server that reports every minute,
/// silence is the alarm; for an error count it is the best news there is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Missing {
    #[default]
    Grey,
    Warn,
    Crit,
}

fn default_refresh() -> u64 {
    DEFAULT_REFRESH_SECS
}

fn default_screen() -> u8 {
    DEFAULT_SCREEN
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_period() -> String {
    "1h".into()
}

fn default_scale() -> f64 {
    1.0
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "no config at {} — start from config.example.toml",
                    path.display()
                )
            } else {
                format!("could not read {}: {err}", path.display())
            }
        })?;
        Config::parse(&text).map_err(|err| format!("{}: {err}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let mut config: Config = toml::from_str(text).map_err(|err| err.to_string())?;
        config.sentry.base_url = config.sentry.base_url.trim_end_matches('/').to_string();
        config.validate()?;
        Ok(config)
    }

    pub fn metrics(&self) -> impl Iterator<Item = &Metric> {
        self.sections
            .iter()
            .flat_map(|section| section.metrics.iter())
    }

    fn validate(&self) -> Result<(), String> {
        let mut problems = Vec::new();

        self.sentry.validate(&mut problems);
        self.device.validate(&mut problems);
        if self.refresh_secs < MIN_REFRESH_SECS {
            problems.push(format!("refresh_secs must be at least {MIN_REFRESH_SECS}"));
        }
        if self.metrics().next().is_none() {
            problems.push("there are no metrics: add a [[section]] with [[section.metric]]".into());
        }
        for (index, section) in self.sections.iter().enumerate() {
            section.validate(index + 1, &mut problems);
        }
        if let Err(err) = layout::plan(&self.section_sizes()) {
            problems.push(err);
        }

        match problems.is_empty() {
            true => Ok(()),
            false => Err(problems.join("\n")),
        }
    }

    pub fn section_sizes(&self) -> Vec<usize> {
        self.sections
            .iter()
            .map(|section| section.metrics.len())
            .collect()
    }
}

impl Sentry {
    fn validate(&self, problems: &mut Vec<String>) {
        if !(self.base_url.starts_with("https://") || self.base_url.starts_with("http://")) {
            problems.push("sentry.base_url must start with https://".into());
        }
        if self.org.trim().is_empty() {
            problems.push("sentry.org is empty".into());
        }
    }
}

impl DeviceConfig {
    fn validate(&self, problems: &mut Vec<String>) {
        if !(1..=divoomkit::SCREEN_COUNT).contains(&self.screen) {
            problems.push(format!(
                "device.screen is a number from 1 to {}",
                divoomkit::SCREEN_COUNT
            ));
        }
        if let Some(ip) = &self.ip
            && ip.parse::<std::net::IpAddr>().is_err()
        {
            problems.push(format!("device.ip is not an IP address: {ip:?}"));
        }
    }

    /// The screen as the device numbers it, from zero.
    pub fn screen_index(&self) -> u8 {
        self.screen - 1
    }
}

impl Section {
    fn validate(&self, number: usize, problems: &mut Vec<String>) {
        let at = format!("section {number}");
        check_text(&at, "title", &self.title, Some(MAX_TITLE), problems);
        if self.metrics.is_empty() {
            problems.push(format!("{at}: has no metrics"));
        }
        for (index, metric) in self.metrics.iter().enumerate() {
            metric.validate(&format!("{at}, metric {}", index + 1), problems);
        }
    }
}

impl Metric {
    fn validate(&self, at: &str, problems: &mut Vec<String>) {
        check_text(at, "label", &self.label, None, problems);
        if self.label.trim().is_empty() {
            problems.push(format!("{at}: label is empty"));
        }
        let room = render::label_room(&self.unit);
        let width = render::text_px(&self.label);
        if width > room {
            problems.push(format!(
                "{at}: label {:?} is {width} px wide, and with room for a value like {} \
                 it may take {room} px ({} characters)",
                self.label,
                render::widest_value(&self.unit),
                render::chars_in(room)
            ));
        }
        let at = format!("{at} ({})", self.label);
        if self.unit.chars().count() > MAX_UNIT || !divoomkit::font::covers(&self.unit) {
            problems.push(format!(
                "{at}: unit is at most {MAX_UNIT} characters the panel can draw"
            ));
        }
        if self.dataset.trim().is_empty() {
            problems.push(format!("{at}: dataset is empty"));
        }
        if self.field.trim().is_empty() {
            problems.push(format!("{at}: field is empty"));
        }
        if !valid_period(&self.period) {
            problems.push(format!(
                "{at}: period is a number and one of s, m, h, d, w — not {:?}",
                self.period
            ));
        }
        if !self.scale.is_finite() || self.scale == 0.0 {
            problems.push(format!("{at}: scale must be a non-zero number"));
        }
        if !self.thresholds_in_order() {
            problems.push(format!(
                "{at}: warn must come before crit in the direction things get worse"
            ));
        }
    }

    fn thresholds_in_order(&self) -> bool {
        match (self.warn, self.crit, self.worse) {
            (Some(warn), Some(crit), Worse::Higher) => warn <= crit,
            (Some(warn), Some(crit), Worse::Lower) => warn >= crit,
            _ => true,
        }
    }
}

fn check_text(at: &str, what: &str, text: &str, max: Option<usize>, problems: &mut Vec<String>) {
    if let Some(max) = max
        && text.chars().count() > max
    {
        problems.push(format!(
            "{at}: {what} {text:?} is longer than {max} characters"
        ));
    }
    // The font draws an unknown character as "?", and a panel of question marks
    // looks like a fault of the device rather than of the config.
    if !divoomkit::font::covers(text) {
        problems.push(format!(
            "{at}: {what} {text:?} has characters the panel cannot draw"
        ));
    }
}

pub fn valid_period(period: &str) -> bool {
    let Some(unit) = period.chars().last() else {
        return false;
    };
    let digits = &period[..period.len() - unit.len_utf8()];
    "smhdw".contains(unit)
        && !digits.is_empty()
        && digits.chars().all(|digit| digit.is_ascii_digit())
        && digits.parse::<u64>().is_ok_and(|number| number > 0)
}

/// `~/` is spelled in configs by people, and only a shell would expand it.
pub fn expand_home(path: &Path, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const MINIMAL: &str = r#"
[sentry]
base_url = "https://de.sentry.io/"
org = "acme"

[[section]]
title = "APP"

[[section.metric]]
label = "ERR"
dataset = "errors"
field = "count()"
"#;

    pub fn minimal() -> Config {
        Config::parse(MINIMAL).unwrap()
    }

    fn with_metric(extra: &str) -> Result<Config, String> {
        Config::parse(&format!("{MINIMAL}{extra}"))
    }

    #[test]
    fn the_example_config_is_valid() {
        let config = Config::parse(include_str!("../config.example.toml")).unwrap();
        assert_eq!(config.sections.len(), 3);
        assert_eq!(config.sentry.org, "my-org");
    }

    #[test]
    fn fills_in_the_defaults() {
        let config = minimal();
        let metric = &config.sections[0].metrics[0];

        assert_eq!(config.refresh_secs, 60);
        assert_eq!(config.device.screen, 1);
        assert_eq!(config.device.screen_index(), 0);
        assert_eq!(config.device.port, DEFAULT_PORT);
        assert_eq!(metric.period, "1h");
        assert_eq!(metric.scale, 1.0);
        assert_eq!(metric.worse, Worse::Higher);
        assert_eq!(metric.missing, Missing::Grey);
        assert_eq!(metric.query, "");
    }

    #[test]
    fn drops_the_trailing_slash_of_the_base_url() {
        assert_eq!(minimal().sentry.base_url, "https://de.sentry.io");
    }

    #[test]
    fn reads_every_option_of_a_metric() {
        let config = with_metric(
            r#"
[[section.metric]]
label = "DISK"
dataset = "logs"
query = "host:a"
field = "min(tags[free,number])"
project = "42"
period = "15m"
unit = "%"
scale = 100
warn = 20
crit = 10
worse = "lower"
missing = "crit"
active_when = "active:a"
"#,
        )
        .unwrap();
        let metric = &config.sections[0].metrics[1];

        assert_eq!(metric.worse, Worse::Lower);
        assert_eq!(metric.missing, Missing::Crit);
        assert_eq!(metric.project.as_deref(), Some("42"));
        assert_eq!(metric.active_when.as_deref(), Some("active:a"));
        assert_eq!(metric.scale, 100.0);
    }

    #[test]
    fn refuses_an_unknown_key() {
        let err = with_metric("\n[[section.metric]]\nlabel = \"X\"\ndataset = \"errors\"\nfield = \"count()\"\ntreshold = 3\n").unwrap_err();
        assert!(err.contains("treshold"), "{err}");
    }

    #[test]
    fn refuses_thresholds_in_the_wrong_order() {
        let err = with_metric(
            "\n[[section.metric]]\nlabel = \"X\"\ndataset = \"errors\"\nfield = \"count()\"\nwarn = 10\ncrit = 5\n",
        )
        .unwrap_err();
        assert!(err.contains("warn must come before crit"), "{err}");

        let lower = with_metric(
            "\n[[section.metric]]\nlabel = \"X\"\ndataset = \"errors\"\nfield = \"count()\"\nwarn = 10\ncrit = 5\nworse = \"lower\"\n",
        );
        assert!(lower.is_ok());
    }

    #[test]
    fn takes_a_label_with_a_space_as_long_as_its_value_fits() {
        let metric = |label: &str, unit: &str| {
            with_metric(&format!(
                "\n[[section.metric]]\nlabel = \"{label}\"\ndataset = \"spans\"\nfield = \"p95(span.duration)\"\nunit = \"{unit}\"\n"
            ))
        };
        assert!(metric("API P95", "ms").is_ok());
        assert!(metric("FRONT LCP", "ms").is_ok());
        assert!(metric("TWELVE CHARS", "ms").is_ok());
        assert!(metric("THIRTEEN CHRS", "ms").is_err());
        assert!(metric("FOURTEEN CHARS", "").is_ok());
        assert!(metric("FIFTEEN CHARS X", "").is_err());
    }

    #[test]
    fn refuses_what_the_panel_cannot_draw() {
        let long = with_metric(
            "\n[[section.metric]]\nlabel = \"MUCH TOO LONG A LABEL\"\ndataset = \"errors\"\nfield = \"count()\"\n",
        )
        .unwrap_err();
        assert!(long.contains("125 px wide"), "{long}");
        assert!(long.contains("it may take 88 px (14 characters)"), "{long}");

        let glyph = with_metric(
            "\n[[section.metric]]\nlabel = \"A_B\"\ndataset = \"errors\"\nfield = \"count()\"\nunit = \"µs\"\n",
        )
        .unwrap_err();
        assert!(glyph.contains("cannot draw"), "{glyph}");
        assert!(glyph.contains("unit"), "{glyph}");
    }

    #[test]
    fn names_every_problem_at_once() {
        let err = Config::parse(
            r#"
refresh_secs = 1
[sentry]
base_url = "de.sentry.io"
org = ""
[device]
screen = 9
ip = "nope"
[[section]]
title = "EMPTY"
"#,
        )
        .unwrap_err();

        for expected in [
            "refresh_secs",
            "base_url",
            "org",
            "device.screen",
            "device.ip",
            "no metrics",
            "has no metrics",
        ] {
            assert!(err.contains(expected), "{expected} missing from {err}");
        }
    }

    #[test]
    fn refuses_a_bad_period_and_scale() {
        let err = with_metric(
            "\n[[section.metric]]\nlabel = \"X\"\ndataset = \"\"\nfield = \" \"\nperiod = \"5 min\"\nscale = 0\n",
        )
        .unwrap_err();
        for expected in ["period", "scale", "dataset is empty", "field is empty"] {
            assert!(err.contains(expected), "{expected} missing from {err}");
        }
    }

    #[test]
    fn refuses_more_rows_than_the_screen_holds() {
        let mut text = MINIMAL.to_string();
        for _ in 0..20 {
            text.push_str(
                "\n[[section.metric]]\nlabel = \"X\"\ndataset = \"errors\"\nfield = \"count()\"\n",
            );
        }
        let err = Config::parse(&text).unwrap_err();
        assert!(err.contains("do not fit"), "{err}");
    }

    #[test]
    fn knows_a_period() {
        for good in ["5m", "1h", "24h", "30s", "7d", "2w"] {
            assert!(valid_period(good), "{good}");
        }
        for bad in ["", "m", "5", "5y", "-5m", "0m", "5 m", "1.5h", "5м"] {
            assert!(!valid_period(bad), "{bad}");
        }
    }

    #[test]
    fn expands_the_home_directory() {
        let home = Path::new("/home/someone");
        assert_eq!(
            expand_home(Path::new("~/.config/token"), Some(home)),
            PathBuf::from("/home/someone/.config/token")
        );
        assert_eq!(
            expand_home(Path::new("/etc/token"), Some(home)),
            PathBuf::from("/etc/token")
        );
        assert_eq!(
            expand_home(Path::new("~/token"), None),
            PathBuf::from("~/token")
        );
    }

    #[test]
    fn reports_a_missing_and_a_broken_file() {
        let dir = std::env::temp_dir().join(format!("sentrypanel-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let missing = Config::load(&dir.join("absent.toml")).unwrap_err();
        assert!(missing.contains("config.example.toml"), "{missing}");

        let broken = dir.join("broken.toml");
        std::fs::write(&broken, "[sentry\n").unwrap();
        let err = Config::load(&broken).unwrap_err();
        assert!(err.contains("broken.toml"), "{err}");

        let good = dir.join("good.toml");
        std::fs::write(&good, MINIMAL).unwrap();
        assert!(Config::load(&good).is_ok());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
