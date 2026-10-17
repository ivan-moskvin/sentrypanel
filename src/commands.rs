//! The commands that do one thing and leave.

use std::path::Path;

use divoomkit::{Discover, discover};

use crate::config::Config;
use crate::sentry::{self, Client, FetchError, Reading};
use crate::state::State;
use crate::{daemon, demo, device, render, screen, service, table, token, view};

pub fn once(config_path: &Path) -> Result<(), String> {
    let config = Config::load(config_path)?;
    let state = fetched(&config)?;

    print!("{}", table::render(&view::build(&config.sections, &state)));
    match state.failure {
        Some(err) => Err(err.to_string()),
        None => Ok(()),
    }
}

pub fn preview(config_path: &Path, file: &Path, demo: bool) -> Result<(), String> {
    let (config, state) = match demo {
        true => demo_state(config_path)?,
        false => {
            let config = Config::load(config_path)?;
            let state = fetched(&config)?;
            (config, state)
        }
    };

    let frame = render::draw(&view::build(&config.sections, &state))?;
    std::fs::write(file, frame.bytes())
        .map_err(|err| format!("could not write {}: {err}", file.display()))?;
    if let Some(err) = &state.failure {
        eprintln!("{err}");
    }
    Ok(())
}

/// The demo draws the user's own sections when there is a config, so the
/// layout can be judged before a token exists; otherwise the example's.
fn demo_state(config_path: &Path) -> Result<(Config, State), String> {
    let config = match config_path.exists() {
        true => Config::load(config_path)?,
        false => Config::parse(include_str!("../config.example.toml"))?,
    };
    let mut state = State::default();
    state.record(Ok(demo::readings(&config.sections)), chrono::Local::now());
    Ok((config, state))
}

pub fn fetched(config: &Config) -> Result<State, String> {
    let home = dirs::home_dir();
    let token = token::resolve(
        &config.sentry,
        std::env::var(token::ENV).ok(),
        home.as_deref(),
    )?;
    let client = Client::new(&config.sentry, token);

    let mut state = State::default();
    state.record(refresh(&client, config), chrono::Local::now());
    Ok(state)
}

pub fn refresh(client: &Client, config: &Config) -> Result<Vec<Vec<Reading>>, FetchError> {
    sentry::collect(client, &sentry::plan(&config.sections))
}

pub fn devices() -> Result<(), String> {
    let found = discover(Discover::new()).map_err(|err| device::describe(&err))?;
    println!("Divoom devices on the network:");
    print!("{}", device::list(&found));
    Ok(())
}

pub fn screen(config_path: &Path, asked: Option<&str>, lock: &Path) -> Result<(), String> {
    let config = Config::load(config_path)?;
    let Some(asked) = asked else {
        println!(
            "The panel is on screen {} of {}",
            config.device.screen,
            divoomkit::SCREEN_COUNT
        );
        return Ok(());
    };

    let number = screen::parse(asked)?;
    let text = std::fs::read_to_string(config_path).map_err(|err| err.to_string())?;
    let edited = screen::set(&text, number)?;
    std::fs::write(config_path, edited)
        .map_err(|err| format!("could not write {}: {err}", config_path.display()))?;
    println!("The panel moves to screen {number}");

    // The running instance holds the old screen; once it has given it back,
    // the service starts a new one that reads the new number.
    if daemon::stop(lock) {
        match service::installed() {
            true => println!("The service restarts it on the new screen"),
            false => println!("It was stopped; start it again: sentrypanel run"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::MINIMAL;
    use crate::testserver::TestServer;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir =
                std::env::temp_dir().join(format!("sentrypanel-cmd-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        /// A config pointing at the local server, with a token file of its own.
        fn config(&self, base_url: &str) -> PathBuf {
            let token = self.path("token");
            std::fs::write(&token, "t").unwrap();
            std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();

            let text = MINIMAL.replace("https://de.sentry.io/", base_url).replace(
                "org = \"acme\"",
                &format!("org = \"acme\"\ntoken_file = {:?}", token),
            );
            let path = self.path("config.toml");
            std::fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn is_gif(path: &Path) -> bool {
        std::fs::read(path).unwrap().starts_with(b"GIF89a")
    }

    #[test]
    fn draws_the_demo_without_a_config_or_a_token() {
        let scratch = Scratch::new("demo");
        let out = scratch.path("demo.gif");

        preview(&scratch.path("absent.toml"), &out, true).unwrap();
        assert!(is_gif(&out));
    }

    #[test]
    fn draws_the_demo_with_the_sections_of_the_config() {
        let scratch = Scratch::new("demo-config");
        let config = scratch.config("http://127.0.0.1:1");
        let out = scratch.path("demo.gif");

        preview(&config, &out, true).unwrap();
        assert!(is_gif(&out));
    }

    #[test]
    fn prints_and_draws_real_numbers() {
        let server = TestServer::answering(200, r#"{"data":[{"count()":6}]}"#);
        let scratch = Scratch::new("real");
        let config = scratch.config(&server.url());
        let out = scratch.path("panel.gif");

        once(&config).unwrap();
        preview(&config, &out, false).unwrap();
        assert!(is_gif(&out));
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn fails_once_when_sentry_refuses_the_token() {
        let server = TestServer::answering(401, "");
        let scratch = Scratch::new("auth");
        let config = scratch.config(&server.url());

        let err = once(&config).unwrap_err();
        assert!(err.contains("refused the token"), "{err}");

        let out = scratch.path("panel.gif");
        preview(&config, &out, false).unwrap();
        assert!(is_gif(&out), "the failure is drawn, not hidden");
    }

    #[test]
    fn moves_the_panel_to_another_screen() {
        let scratch = Scratch::new("screen");
        let config = scratch.config("http://127.0.0.1:1");
        let lock = scratch.path("absent.pid");

        screen(&config, None, &lock).unwrap();
        screen(&config, Some("3"), &lock).unwrap();
        assert_eq!(Config::load(&config).unwrap().device.screen, 3);

        assert!(screen(&config, Some("9"), &lock).is_err());
        assert!(screen(&scratch.path("absent.toml"), Some("2"), &lock).is_err());
    }

    #[test]
    fn says_where_the_preview_could_not_be_written() {
        let scratch = Scratch::new("unwritable");
        let err = preview(
            &scratch.path("absent.toml"),
            &scratch.path("no/such/dir.gif"),
            true,
        )
        .unwrap_err();
        assert!(err.contains("could not write"), "{err}");
    }
}
