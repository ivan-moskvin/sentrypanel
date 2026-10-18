//! The commands that do one thing and leave.

use std::path::Path;

use divoomkit::{Discover, discover};

use crate::config::Config;
use crate::sentry::{self, Client, FetchError, Reading};
use crate::state::State;
use crate::{
    bundle, daemon, demo, device, paths, render, run, screen, service, table, token, view,
};

const EXAMPLE: &str = include_str!("../config.example.toml");

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
        false => Config::parse(EXAMPLE)?,
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
    // a new one reads the new number.
    service::restart(lock, config_path)
}

pub fn stop(lock: &Path) -> Result<(), String> {
    match daemon::stop(lock) {
        true => println!("The panel is stopped and the screen given back"),
        false => println!("The panel is not running"),
    }
    if service::installed() && cfg!(unix) {
        println!("The service starts it again; to keep it off: sentrypanel service uninstall");
    }
    Ok(())
}

/// `config` names the file every command reads; `config init` puts the
/// example there. An existing config is never overwritten.
pub fn config(path: &Path, init: bool) -> Result<(), String> {
    if !init {
        println!("{}", path.display());
        if !path.exists() {
            eprintln!("It is not there yet: sentrypanel config init");
        }
        return Ok(());
    }

    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path);
    let mut file = match file {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!("{} already exists — left as it is", path.display()));
        }
        Err(err) => return Err(format!("could not write {}: {err}", path.display())),
    };
    std::io::Write::write_all(&mut file, EXAMPLE.as_bytes())
        .map_err(|err| format!("could not write {}: {err}", path.display()))?;

    println!("Created {}", path.display());
    println!("Set sentry.org and the sections in it, then check it: sentrypanel once");
    Ok(())
}

/// The token is read the way the panel reads it, so a bundle carries the one
/// that works here.
pub fn export(config_path: &Path, out: &Path) -> Result<(), String> {
    let config = Config::load(config_path)?;
    let text = std::fs::read_to_string(config_path)
        .map_err(|err| format!("could not read {}: {err}", config_path.display()))?;
    let home = dirs::home_dir();
    let token = token::resolve(
        &config.sentry,
        std::env::var(token::ENV).ok(),
        home.as_deref(),
    )?;
    let known = device::recall_device(&paths::file(run::KNOWN)?);

    bundle::export(&text, &token, known.as_ref(), out)?;
    println!("Wrote {} — readable by you only", out.display());
    println!("It holds the Sentry token: carry it over yourself, never by a link,");
    println!("and delete it once imported. On the other machine:");
    println!("  sentrypanel config import FILE   or the install script with SENTRYPANEL_BUNDLE");
    Ok(())
}

pub fn import(file: &Path, config_path: &Path) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("no home directory")?;
    let imported = bundle::import(file, config_path, &home)?;
    if let Some(backup) = &imported.backup {
        println!("The previous config is kept as {}", backup.display());
    }
    println!("Wrote {}", imported.config.display());
    println!("Wrote the token to {}", imported.token.display());
    println!("Check it: sentrypanel once; then delete {}", file.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::MINIMAL;
    use crate::testserver::TestServer;
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
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
            }

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
    fn creates_the_config_from_the_example_once() {
        let scratch = Scratch::new("config-init");
        let path = scratch.path("nested/config.toml");

        config(&path, false).unwrap();
        config(&path, true).unwrap();
        assert!(Config::load(&path).is_ok(), "the example is a valid config");

        std::fs::write(&path, "mine").unwrap();
        let err = config(&path, true).unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine");
    }

    #[test]
    fn the_example_names_nothing_of_one_machine() {
        let example = Config::parse(EXAMPLE).unwrap();
        assert_eq!(example.device.ip, None);
        let token = example.sentry.token_file.unwrap();
        assert!(token.starts_with("~"), "{}", token.display());
    }

    #[test]
    fn stops_nothing_when_nothing_runs() {
        let scratch = Scratch::new("stop");
        stop(&scratch.path("absent.pid")).unwrap();
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
