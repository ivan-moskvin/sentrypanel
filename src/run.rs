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
pub const FACE: &str = "clock-face.json";
pub const KNOWN: &str = "device.json";

/// How often the loop looks up from its sleep — for a signal, a lost pid file,
/// or a refresh that is due. Sentry is asked only every `refresh_secs`. Short,
/// because Windows gives a process seconds to leave at a logoff.
const POLL: Duration = Duration::from_secs(1);

/// How long to wait before looking for a device that could not be reached.
/// Nothing restarts the panel on Windows, so `run` does not give up by itself.
const RETRY: Duration = Duration::from_secs(60);

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

    let outcome = keep_serving(&config, &client, &lock);
    daemon::drop_lock(&lock);
    daemon::finished();
    outcome
}

/// A device switched off for the night, a network not up yet at login, a
/// lease that moved the device elsewhere: none of it ends the panel. It is
/// said once and tried again until somebody stops us.
fn keep_serving(config: &Config, client: &Client, lock: &Path) -> Result<(), String> {
    let mut last = String::new();
    loop {
        match serve(config, client, lock) {
            Ok(()) => return Ok(()),
            Err(err) if err != last => {
                log(&format!("{err}; trying again every {}s", RETRY.as_secs()));
                last = err;
            }
            Err(_) => {}
        }
        if !pause(RETRY, lock) {
            return Ok(());
        }
    }
}

/// Sleeps unless told to stop; answers whether to go on.
fn pause(length: Duration, lock: &Path) -> bool {
    let until = Instant::now() + length;
    while Instant::now() < until {
        if daemon::stopping() || !daemon::owns_lock(lock) {
            return false;
        }
        std::thread::sleep(POLL);
    }
    true
}

fn serve(config: &Config, client: &Client, lock: &Path) -> Result<(), String> {
    let memory = paths::file(KNOWN)?;
    let bridge = bridge(config, &memory)?;
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
    let mut known_ip = bridge.device().ip();

    let outcome = bridge.run(
        &Pacing {
            poll: POLL,
            ..Default::default()
        },
        || {
            if daemon::stopping() || !daemon::owns_lock(lock) {
                return Ok(Tick::Stop);
            }
            // The bridge follows a device to a new address on its own; the next
            // start should begin there too.
            let device = bridge.device();
            if device.ip() != known_ip {
                known_ip = device.ip();
                log(&format!("the device moved to {known_ip}"));
                let _ = device::remember_device(&memory, &device);
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

fn bridge(config: &Config, memory: &Path) -> Result<Bridge, String> {
    let located = device::locate(&config.device, memory)?;
    if located.remembered {
        log(&format!(
            "no device seen on the network; using the address it was last found at, {}",
            located.device.ip()
        ));
    }
    let screen = config.device.screen_index();
    let key = device::key(&located, screen);
    let faces = paths::file(FACE)?;

    let mut builder = Bridge::builder(located.device)
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
    crate::log::line(message);
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
