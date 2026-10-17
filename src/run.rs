//! `run`: keeps the screen updated until told to stop.

use std::path::Path;
use std::time::{Duration, Instant};

use divoomkit::{Bridge, Pacing, Tick};

use crate::commands::refresh;
use crate::config::Config;
use crate::sentry::{Client, FetchError, Reading};
use crate::state::State;
use crate::{daemon, device, paths, render, token, view};

pub const LOCK: &str = "sentrypanel.pid";
const FACE: &str = "clock-face.json";

/// How often the loop looks up from its sleep — for a signal, a lost pid file,
/// or a refresh that is due. Sentry is asked only every `refresh_secs`.
const POLL: Duration = Duration::from_secs(5);

pub fn run(config_path: &Path) -> Result<(), String> {
    let config = Config::load(config_path)?;
    let home = dirs::home_dir();
    let token = token::resolve(
        &config.sentry,
        std::env::var(token::ENV).ok(),
        home.as_deref(),
    )?;
    let client = Client::new(&config.sentry, token);

    let lock = paths::file(LOCK)?;
    daemon::take_lock(&lock)?;
    daemon::catch_signals();

    let outcome = serve(&config, &client, &lock);
    daemon::drop_lock(&lock);
    outcome
}

fn serve(config: &Config, client: &Client, lock: &Path) -> Result<(), String> {
    let bridge = bridge(config)?;
    log(&format!(
        "panel on screen {} of {}, refreshed every {}s",
        config.device.screen,
        bridge.device().ip(),
        config.refresh_secs
    ));

    let every = Duration::from_secs(config.refresh_secs);
    let mut state = State::default();
    let mut due = Instant::now();
    let mut last_problems = Vec::new();

    let outcome = bridge.run(
        &Pacing {
            poll: POLL,
            ..Default::default()
        },
        || {
            if daemon::stopping() || !daemon::owns_lock(lock) {
                return Ok(Tick::Stop);
            }
            if Instant::now() >= due {
                due = Instant::now() + every;
                let result = refresh(client, config);
                report(&result, &mut last_problems);
                state.record(result, chrono::Local::now());
            }
            let frame = render::draw(&view::build(&config.sections, &state))
                .map_err(divoomkit::Error::Malformed)?;
            // The bridge skips a frame identical to the last one, so offering
            // it on every tick costs nothing.
            Ok(Tick::Now(frame))
        },
        |err| log(&device::describe(err)),
    );

    outcome.map_err(|err| format!("giving up on the device: {}", device::describe(&err)))
}

fn bridge(config: &Config) -> Result<Bridge, String> {
    let device = device::locate(&config.device)?;
    let screen = config.device.screen_index();
    let key = device::key(&device, screen);
    let faces = paths::file(FACE)?;

    let mut builder = Bridge::builder(device)
        .screen(screen)
        .port(config.device.port);
    if let Some(face) = device::recall(&faces, &key) {
        builder = builder.restore_to(face);
    }
    let bridge = builder.build().map_err(|err| device::describe(&err))?;

    if let Some(face) = bridge.restores_to() {
        device::remember(&faces, &key, face)?;
    }
    Ok(bridge)
}

/// A failure is logged when it starts and when it ends, not on every refresh:
/// a night without network would otherwise be a thousand identical lines.
fn report(result: &Result<Vec<Vec<Reading>>, FetchError>, last: &mut Vec<String>) {
    let problems = problems(result);
    if problems == *last {
        return;
    }
    if problems.is_empty() {
        log("Sentry answers again");
    }
    for problem in &problems {
        log(problem);
    }
    *last = problems;
}

fn problems(result: &Result<Vec<Vec<Reading>>, FetchError>) -> Vec<String> {
    match result {
        Err(err) => vec![err.to_string()],
        Ok(readings) => readings
            .iter()
            .flatten()
            .filter_map(|reading| reading.value.as_ref().err().cloned())
            .collect(),
    }
}

fn log(message: &str) {
    eprintln!(
        "{} {message}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(value: Result<Option<f64>, String>) -> Reading {
        Reading {
            value,
            active: None,
        }
    }

    #[test]
    fn lists_the_problems_of_a_refresh() {
        assert_eq!(
            problems(&Err(FetchError::RateLimited)),
            ["Sentry rate limit hit (HTTP 429)"]
        );
        let readings = vec![vec![
            reading(Ok(Some(1.0))),
            reading(Err("bad field".into())),
        ]];
        assert_eq!(problems(&Ok(readings)), ["bad field"]);
    }

    #[test]
    fn remembers_what_it_reported() {
        let mut last = Vec::new();
        report(&Err(FetchError::Auth(401)), &mut last);
        assert_eq!(last.len(), 1);

        report(&Ok(vec![vec![reading(Ok(None))]]), &mut last);
        assert!(last.is_empty());
    }
}
