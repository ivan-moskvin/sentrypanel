//! `config export` and `config import`: the config and the token in one file,
//! to set up another machine with a single command.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{Config, expand_home};
use crate::device::Known;
use crate::token::Token;

const FORMAT: u32 = 1;

/// Where the token goes on the machine the bundle is imported on: inside the
/// home directory on every system, which is what Windows asks of a token file.
pub const TOKEN_FILE: &str = "~/.config/sentrypanel/token";

pub const DEFAULT_NAME: &str = "sentrypanel-bundle";

const HEADER: &str = "# A sentrypanel bundle: a config and the Sentry token it reads with.\n\
# Keep it to yourself and delete it once imported: sentrypanel config import FILE\n\n";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: u32,
    config: String,
    token: String,
}

/// The token is a secret: neither the bundle nor its parts are ever printed.
impl std::fmt::Debug for Bundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bundle(***)")
    }
}

#[derive(Debug)]
pub struct Imported {
    pub config: PathBuf,
    pub token: PathBuf,
    pub backup: Option<PathBuf>,
}

/// The config as it should read on any machine: the token where every system
/// lets it live, and the device also named, so that it is found where its
/// address is a different one.
pub fn portable(text: &str, known: Option<&Known>) -> Result<String, String> {
    let mut document: toml_edit::DocumentMut = text.parse().map_err(|err| format!("{err}"))?;

    let sentry = document
        .get_mut("sentry")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or("the config has no [sentry] table")?;
    sentry.insert("token_file", toml_edit::value(TOKEN_FILE));

    let name = known
        .map(|known| known.name.trim())
        .filter(|name| !name.is_empty());
    if let Some(name) = name {
        let device = document
            .entry("device")
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_like_mut()
            .ok_or("[device] in the config is not a table")?;
        if !device.contains_key("name") {
            device.insert("name", toml_edit::value(name));
        }
    }

    let text = document.to_string();
    Config::parse(&text)?;
    Ok(text)
}

pub fn export(
    config_text: &str,
    token: &Token,
    known: Option<&Known>,
    out: &Path,
) -> Result<(), String> {
    let bundle = Bundle {
        format: FORMAT,
        config: portable(config_text, known)?,
        token: token.expose().to_string(),
    };
    let text = toml::to_string(&bundle).map_err(|err| err.to_string())?;
    write_private(out, &format!("{HEADER}{text}"))
}

/// Writes the config where this machine reads it and the token where the
/// config says. A config already there is kept beside it as `.bak`.
pub fn import(bundle: &Path, config: &Path, home: &Path) -> Result<Imported, String> {
    let bundle = read(bundle)?;
    let text = portable(&bundle.config, None)?;
    let token = expand_home(Path::new(TOKEN_FILE), Some(home));

    write_private(&token, &format!("{}\n", bundle.token.trim()))?;

    let backup = match std::fs::read_to_string(config) {
        Ok(old) if old != text => {
            let backup = config.with_extension("toml.bak");
            std::fs::write(&backup, old)
                .map_err(|err| format!("could not write {}: {err}", backup.display()))?;
            Some(backup)
        }
        _ => None,
    };
    create_parent(config)?;
    std::fs::write(config, text)
        .map_err(|err| format!("could not write {}: {err}", config.display()))?;

    Ok(Imported {
        config: config.to_path_buf(),
        token,
        backup,
    })
}

fn read(path: &Path) -> Result<Bundle, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("could not read the bundle {}: {err}", path.display()))?;
    // The parser quotes the line it stumbled on, and that line may hold the
    // token: its message is not passed on.
    let bundle: Bundle = toml::from_str(&text)
        .map_err(|_| format!("{} is not a sentrypanel bundle", path.display()))?;
    if bundle.format != FORMAT {
        return Err(format!(
            "{} is a bundle of format {}; this version reads format {FORMAT} — update sentrypanel",
            path.display(),
            bundle.format
        ));
    }
    if bundle.token.trim().is_empty() {
        return Err(format!("{} holds no token", path.display()));
    }
    Ok(bundle)
}

fn create_parent(path: &Path) -> Result<(), String> {
    match path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        Some(dir) => std::fs::create_dir_all(dir)
            .map_err(|err| format!("could not create {}: {err}", dir.display())),
        None => Ok(()),
    }
}

/// Readable by its owner only from the first byte: a file created open and
/// narrowed afterwards could be read in between.
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    create_parent(path)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|err| format!("could not write {}: {err}", path.display()))?;
    // An existing file keeps its mode on open, so it is narrowed as well.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("could not protect {}: {err}", path.display()))?;
    }
    std::io::Write::write_all(&mut file, text.as_bytes())
        .map_err(|err| format!("could not write {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::MINIMAL;
    use crate::token;

    const SECRET: &str = "sntryu_placeholder";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir()
                .join(format!("sentrypanel-bundle-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn mac_config() -> String {
        MINIMAL.replace(
            "org = \"acme\"",
            "org = \"acme\"\ntoken_file = \"/Users/someone/Library/token\"\n\n[device]\nip = \"192.0.2.10\"\nscreen = 2",
        )
    }

    fn known(name: &str) -> Known {
        Known {
            ip: "192.0.2.10".into(),
            id: 7,
            name: name.into(),
        }
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn makes_the_config_portable_and_keeps_the_device() {
        let text = portable(&mac_config(), Some(&known("Times Gate"))).unwrap();
        let config = Config::parse(&text).unwrap();

        assert_eq!(config.sentry.token_file, Some(PathBuf::from(TOKEN_FILE)));
        assert_eq!(config.device.screen, 2);
        assert_eq!(config.device.ip.as_deref(), Some("192.0.2.10"));
        assert_eq!(config.device.name.as_deref(), Some("Times Gate"));
        assert!(!text.contains("Library"), "{text}");
    }

    #[test]
    fn keeps_a_name_the_config_gives() {
        let named = mac_config().replace("screen = 2", "screen = 2\nname = \"Desk\"");
        let text = portable(&named, Some(&known("Times Gate"))).unwrap();
        assert_eq!(
            Config::parse(&text).unwrap().device.name.as_deref(),
            Some("Desk")
        );

        let unnamed = portable(&mac_config(), Some(&known(" "))).unwrap();
        assert_eq!(Config::parse(&unnamed).unwrap().device.name, None);
    }

    #[test]
    fn refuses_to_carry_a_broken_config() {
        assert!(portable("refresh_secs = 60", None).is_err());
        assert!(portable(&mac_config().replace("screen = 2", "screen = 9"), None).is_err());
    }

    #[test]
    fn carries_the_config_and_the_token_to_another_home() {
        let scratch = Scratch::new("round-trip");
        let out = scratch.path("bundle");
        export(
            &mac_config(),
            &Token::new(SECRET),
            Some(&known("Desk")),
            &out,
        )
        .unwrap();

        #[cfg(unix)]
        assert_eq!(mode(&out), 0o600);

        let home = scratch.path("home");
        let config_path = home.join("app/config.toml");
        let imported = import(&out, &config_path, &home).unwrap();
        assert_eq!(imported.token, home.join(".config/sentrypanel/token"));
        assert!(imported.backup.is_none());

        let config = Config::load(&config_path).unwrap();
        assert_eq!(config.device.screen, 2);
        assert_eq!(config.device.name.as_deref(), Some("Desk"));
        let token = token::resolve(&config.sentry, None, Some(&home)).unwrap();
        assert_eq!(token.expose(), SECRET);

        #[cfg(unix)]
        assert_eq!(mode(&imported.token), 0o600);
    }

    #[test]
    fn keeps_the_config_it_replaces() {
        let scratch = Scratch::new("backup");
        let out = scratch.path("bundle");
        export(&mac_config(), &Token::new(SECRET), None, &out).unwrap();

        let home = scratch.path("home");
        let config_path = scratch.path("config.toml");
        std::fs::write(&config_path, "mine").unwrap();

        let imported = import(&out, &config_path, &home).unwrap();
        let backup = imported.backup.unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "mine");

        let again = import(&out, &config_path, &home).unwrap();
        assert!(again.backup.is_none(), "the same config is not backed up");
    }

    #[cfg(unix)]
    #[test]
    fn narrows_a_file_that_was_open_to_others() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = Scratch::new("narrow");
        let out = scratch.path("bundle");
        std::fs::write(&out, "old").unwrap();
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o644)).unwrap();

        export(&mac_config(), &Token::new(SECRET), None, &out).unwrap();
        assert_eq!(mode(&out), 0o600);
    }

    #[test]
    fn refuses_what_is_not_a_bundle_without_quoting_it() {
        let scratch = Scratch::new("refused");
        let home = scratch.path("home");
        let config_path = scratch.path("config.toml");
        let bundle = scratch.path("bundle");

        let cases = [
            format!("token = \"{SECRET}\"\nconfig = "),
            format!("format = 2\nconfig = \"\"\ntoken = \"{SECRET}\""),
            format!("format = 1\nconfig = \"refresh_secs = 60\"\ntoken = \"{SECRET}\""),
            "format = 1\nconfig = \"\"\ntoken = \" \"".to_string(),
        ];
        for case in cases {
            std::fs::write(&bundle, &case).unwrap();
            let err = import(&bundle, &config_path, &home).unwrap_err();
            assert!(!err.contains(SECRET), "the token leaked: {err}");
            assert!(!config_path.exists(), "{case}");
        }

        let err = import(&scratch.path("absent"), &config_path, &home).unwrap_err();
        assert!(err.contains("could not read the bundle"), "{err}");
    }

    #[test]
    fn never_shows_the_token_in_debug_output() {
        let bundle = Bundle {
            format: FORMAT,
            config: String::new(),
            token: SECRET.into(),
        };
        assert!(!format!("{bundle:?}").contains(SECRET));
    }
}
