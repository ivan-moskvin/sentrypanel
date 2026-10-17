//! Where the Sentry token comes from. It is never printed, logged or written
//! anywhere by us.

use std::path::Path;

use crate::config::{Sentry, expand_home};

pub const ENV: &str = "SENTRY_AUTH_TOKEN";

/// The token is a secret, so the type keeps it out of `{:?}` and error messages.
#[derive(Clone)]
pub struct Token(String);

impl Token {
    #[cfg(test)]
    pub fn new(value: impl Into<String>) -> Self {
        Token(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(***)")
    }
}

/// The environment wins: it is how a one-off run overrides the file.
pub fn resolve(sentry: &Sentry, env: Option<String>, home: Option<&Path>) -> Result<Token, String> {
    if let Some(value) = env.map(|value| value.trim().to_string())
        && !value.is_empty()
    {
        return Ok(Token(value));
    }

    let Some(file) = &sentry.token_file else {
        return Err(format!(
            "no Sentry token: set {ENV} or sentry.token_file in the config"
        ));
    };
    read_file(&expand_home(file, home), home)
}

fn read_file(path: &Path, home: Option<&Path>) -> Result<Token, String> {
    check_private(path, home)?;
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("could not read the token file {}: {err}", path.display()))?;
    let value = text.trim();
    if value.is_empty() {
        return Err(format!("the token file {} is empty", path.display()));
    }
    Ok(Token(value.to_string()))
}

/// Refused the way ssh refuses a private key others can read: a token anyone on
/// the machine can copy is a token already leaked.
#[cfg(unix)]
fn check_private(path: &Path, _home: Option<&Path>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("could not read the token file {}: {err}", path.display()))?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(format!(
            "the token file {} can be read by others (mode {mode:o}) — chmod 600 it",
            path.display()
        ));
    }
    Ok(())
}

/// Windows has no mode bits to read. What keeps others out there is the
/// profile: its folders are open to their owner only, so the token has to live
/// inside it.
#[cfg(windows)]
fn check_private(path: &Path, home: Option<&Path>) -> Result<(), String> {
    std::fs::metadata(path)
        .map_err(|err| format!("could not read the token file {}: {err}", path.display()))?;
    match home {
        Some(home) if !inside(path, home) => Err(format!(
            "the token file {} is outside your profile {} — keep it there, where others cannot read it",
            path.display(),
            home.display()
        )),
        _ => Ok(()),
    }
}

/// Both sides are resolved first: Windows spells the same folder by its short
/// name in one variable and by its long one in another.
#[cfg(any(windows, test))]
fn inside(path: &Path, home: &Path) -> bool {
    resolved(path).starts_with(resolved(home))
}

/// The real spelling of the part of the path that exists, the rest as written.
#[cfg(any(windows, test))]
fn resolved(path: &Path) -> std::path::PathBuf {
    path.ancestors()
        .find_map(|known| {
            let real = std::fs::canonicalize(known).ok()?;
            Some(real.join(path.strip_prefix(known).ok()?))
        })
        .unwrap_or_else(|| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sentry(token_file: Option<PathBuf>) -> Sentry {
        Sentry {
            base_url: "https://de.sentry.io".into(),
            org: "acme".into(),
            token_file,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sentrypanel-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str, mode: u32) {
        std::fs::write(path, text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = mode;
    }

    #[test]
    fn prefers_the_environment() {
        let token = resolve(
            &sentry(Some("/nowhere".into())),
            Some(" abc \n".into()),
            None,
        );
        assert_eq!(token.unwrap().expose(), "abc");
    }

    #[test]
    fn ignores_an_empty_environment_variable() {
        let err = resolve(&sentry(None), Some("  ".into()), None).unwrap_err();
        assert!(err.contains(ENV), "{err}");
    }

    #[test]
    fn reads_a_private_file_relative_to_home() {
        let dir = temp_dir("token-ok");
        write(&dir.join("token"), "sntrys_secret\n", 0o600);

        let token = resolve(&sentry(Some("~/token".into())), None, Some(&dir)).unwrap();
        assert_eq!(token.expose(), "sntrys_secret");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_file_others_can_read() {
        let dir = temp_dir("token-open");
        let path = dir.join("token");
        write(&path, "sntrys_secret", 0o644);

        let err = resolve(&sentry(Some(path)), None, None).unwrap_err();
        assert!(err.contains("chmod 600"), "{err}");
        assert!(!err.contains("sntrys_secret"), "the token leaked: {err}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_an_empty_or_missing_file() {
        let dir = temp_dir("token-empty");
        let path = dir.join("token");
        write(&path, "\n", 0o600);

        let empty = resolve(&sentry(Some(path)), None, None).unwrap_err();
        assert!(empty.contains("empty"), "{empty}");

        let missing = resolve(&sentry(Some(dir.join("absent"))), None, None).unwrap_err();
        assert!(missing.contains("absent"), "{missing}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn knows_a_file_inside_the_profile_from_one_outside() {
        let home = temp_dir("token-home");
        let elsewhere = temp_dir("token-elsewhere");

        assert!(inside(&home.join("token"), &home));
        assert!(inside(&home.join("nested/token"), &home));
        assert!(!inside(&elsewhere.join("token"), &home));

        std::fs::remove_dir_all(&home).unwrap();
        std::fs::remove_dir_all(&elsewhere).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn refuses_a_file_outside_the_profile() {
        let home = temp_dir("token-profile");
        let elsewhere = temp_dir("token-outside");
        let path = elsewhere.join("token");
        write(&path, "sntrys_secret", 0o600);

        let err = resolve(&sentry(Some(path)), None, Some(&home)).unwrap_err();
        assert!(err.contains("outside your profile"), "{err}");
        assert!(!err.contains("sntrys_secret"), "the token leaked: {err}");

        std::fs::remove_dir_all(&home).unwrap();
        std::fs::remove_dir_all(&elsewhere).unwrap();
    }

    #[test]
    fn never_shows_the_token_in_debug_output() {
        let printed = format!("{:?}", Token::new("sntrys_secret"));
        assert!(!printed.contains("secret"), "{printed}");
    }
}
