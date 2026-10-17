//! `check` and `update`: the latest release on GitHub, and replacing ourselves
//! with it.
//!
//! We update the same way we were installed: download the ready binary from a
//! release and replace ourselves with it. Neither a toolchain nor a clone of
//! the repository is needed on the machine.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{paths, run, service, version};

const REPOSITORY: &str = "ivan-moskvin/sentrypanel";

const NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Where releases are looked for. A struct rather than a constant so that
/// updating can be tested against a local server without publishing anything.
#[derive(Debug, Clone)]
pub struct Source {
    api: String,
    downloads: String,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            api: format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
            downloads: format!("https://github.com/{REPOSITORY}/releases/download/"),
        }
    }
}

pub fn check() -> Result<(), String> {
    let latest = latest_version(&Source::default())?;
    let installed = version();
    if is_newer(&installed, &latest) {
        println!("{installed} installed, {latest} is out — update with: sentrypanel update");
    } else if parse_version(&installed).is_none() {
        println!("Not a release build, the latest version is {latest}");
    } else {
        println!("The latest version is installed: {installed}");
    }
    Ok(())
}

/// Downloads the binary of the latest release, replaces us with it and has the
/// running panel started again from the new one.
pub fn update(config: &Path) -> Result<(), String> {
    let source = Source::default();
    let latest = latest_version(&source)?;

    let installed = version();
    let a_release = parse_version(&installed).is_some();
    if a_release && !is_newer(&installed, &latest) {
        println!("Already the latest version: {installed}");
        return Ok(());
    }

    let exe = self_path()?;
    match a_release {
        true => println!("==> Updating {installed} → {latest}"),
        false => println!("==> Installing {latest}"),
    }
    replace_self(&source, &exe, &latest)?;
    println!("Done: {latest}");

    service::restart(&paths::file(run::LOCK)?, config)
}

/// Downloads the release binary next to the current one and swaps them by
/// renaming: writing over a running file is not allowed, while a rename
/// survives even our own running process.
fn replace_self(source: &Source, exe: &Path, tag: &str) -> Result<(), String> {
    let asset = asset_name();

    let sums = fetch(
        &format!("{}{tag}/checksums.txt", source.downloads),
        DOWNLOAD_TIMEOUT,
    )
    .map_err(|err| format!("could not download the checksums: {}", err))?;
    let sums = String::from_utf8_lossy(&sums);
    let want = checksum_for(&sums, &asset)
        .ok_or_else(|| format!("release {} has no binary for {}", tag, platform()))?;

    let binary = fetch(
        &format!("{}{tag}/{asset}", source.downloads),
        DOWNLOAD_TIMEOUT,
    )
    .map_err(|err| format!("could not download {}: {}", asset, err))?;

    let got: String = Sha256::digest(&binary)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if got != want {
        return Err(format!(
            "the checksum of {} does not match — the file is broken or tampered with",
            asset
        ));
    }

    let staged = with_suffix(exe, ".new");
    write_executable(&staged, &binary)
        .map_err(|err| format!("could not write {}: {}", staged.display(), err))?;

    let previous = with_suffix(exe, ".old");
    let _ = std::fs::remove_file(&previous);
    if let Err(err) = std::fs::rename(exe, &previous) {
        let _ = std::fs::remove_file(&staged);
        return Err(format!("could not move the previous binary away: {}", err));
    }
    if let Err(err) = std::fs::rename(&staged, exe) {
        let _ = std::fs::rename(&previous, exe);
        return Err(format!("could not put the new binary in place: {}", err));
    }
    // Windows will not let a running file be deleted — it goes away on the next
    // update.
    let _ = std::fs::remove_file(&previous);
    Ok(())
}

fn write_executable(path: &Path, data: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn with_suffix(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The file name in the release for the current platform.
///
/// The names follow claudestatus — `<os>_<arch>` as `uname` and install.sh
/// spell them, whatever this compiler calls the platform.
fn asset_name() -> String {
    let name = format!("sentrypanel_{}", platform().replace('/', "_"));
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name
    }
}

fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}/{arch}")
}

/// Looks for a line of the form `<sha256>  <file>` — the sha256sum format.
fn checksum_for(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let sum = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (name == asset && fields.next().is_none()).then(|| sum.to_string())
    })
}

fn latest_version(source: &Source) -> Result<String, String> {
    let body = fetch(&source.api, NETWORK_TIMEOUT)
        .map_err(|err| format!("could not find out the latest version: {}", err))?;

    #[derive(Deserialize)]
    struct Release {
        #[serde(default)]
        tag_name: String,
    }
    let release: Release = serde_json::from_slice(&body)
        .map_err(|err| format!("could not parse the GitHub response: {}", err))?;

    if release.tag_name.is_empty() {
        return Err(format!("{} has no releases yet", REPOSITORY));
    }
    Ok(release.tag_name)
}

fn fetch(url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "sentrypanel")
        .config()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|err| err.to_string())?;

    if response.status() != 200 {
        return Err(format!("{} answered {}", url, response.status()));
    }
    response
        .body_mut()
        .with_config()
        // A release binary is a few megabytes; the default cap is smaller.
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|err| err.to_string())
}

/// The path to our own binary with symlinks resolved: the command is run
/// through a link from ~/.local/bin, but it has to update the real file.
pub fn self_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|err| format!("could not determine our own path: {err}"))?;
    let real = std::fs::canonicalize(&exe)
        .map_err(|err| format!("could not determine our own path: {err}"))?;
    Ok(plain(real))
}

/// Windows resolves a path into its `\\?\C:\...` form, which ends up in the
/// login entry and in messages; the ordinary spelling names the same file.
fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Semver(u32, u32, u32);

/// Reads a tag of the form v1.2.3: a suffix after the dash (-rc1) does not take
/// part in comparing the number.
fn parse_version(value: &str) -> Option<Semver> {
    let value = value.trim().trim_start_matches('v');
    let value = value.split(['-', '+']).next()?;

    let mut fields = value.split('.');
    let mut number = || fields.next()?.parse::<u32>().ok();
    let parsed = Semver(number()?, number()?, number()?);

    fields.next().is_none().then_some(parsed)
}

/// Compares the installed version against a tag. A build made outside a release
/// counts as outdated: it has no number to compare with.
fn is_newer(installed: &str, tag: &str) -> bool {
    match (parse_version(installed), parse_version(tag)) {
        (Some(installed), Some(latest)) => installed < latest,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for GitHub: it serves a release and remembers nothing else.
    /// No test here may reach the real network.
    struct Releases {
        server: std::sync::Arc<tiny_http::Server>,
        thread: Option<std::thread::JoinHandle<()>>,
        base: String,
    }

    impl Releases {
        fn serving(files: Vec<(String, Vec<u8>)>) -> Self {
            let server = std::sync::Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
            let base = format!("http://{}", server.server_addr().to_ip().unwrap());

            let thread = std::thread::spawn({
                let server = std::sync::Arc::clone(&server);
                move || {
                    for request in server.incoming_requests() {
                        let url = request.url().to_string();
                        let found = files.iter().find(|(name, _)| url.ends_with(name.as_str()));
                        let answer = match found {
                            Some((_, data)) => tiny_http::Response::from_data(data.clone()),
                            None => {
                                tiny_http::Response::from_data(Vec::new()).with_status_code(404)
                            }
                        };
                        let _ = request.respond(answer);
                    }
                }
            });

            Self {
                server,
                thread: Some(thread),
                base,
            }
        }

        fn source(&self) -> Source {
            Source {
                api: format!("{}/latest", self.base),
                downloads: format!("{}/download/", self.base),
            }
        }
    }

    impl Drop for Releases {
        fn drop(&mut self) {
            self.server.unblock();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn sha256_of(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// A file standing in for the running binary, in a directory of its own.
    fn staged_binary(name: &str, content: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sentrypanel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("sentrypanel");
        std::fs::write(&exe, content).unwrap();
        exe
    }

    #[test]
    fn reads_the_tag_of_the_latest_release() {
        let releases = Releases::serving(vec![(
            "latest".into(),
            br#"{"tag_name":"v1.4.0"}"#.to_vec(),
        )]);
        assert_eq!(latest_version(&releases.source()).unwrap(), "v1.4.0");
    }

    #[test]
    fn says_so_when_there_are_no_releases_yet() {
        let releases = Releases::serving(vec![("latest".into(), b"{}".to_vec())]);
        let err = latest_version(&releases.source()).unwrap_err();
        assert!(err.contains(REPOSITORY), "{err}");
    }

    #[test]
    fn replaces_itself_with_the_binary_of_the_release() {
        let binary = b"the new binary".to_vec();
        let sums = format!("{}  {}\n", sha256_of(&binary), asset_name());
        let releases = Releases::serving(vec![
            ("checksums.txt".into(), sums.into_bytes()),
            (asset_name(), binary.clone()),
        ]);

        let exe = staged_binary("replace", b"the old binary");
        replace_self(&releases.source(), &exe, "v1.4.0").unwrap();

        assert_eq!(std::fs::read(&exe).unwrap(), binary);
        assert!(
            !exe.with_extension("new").exists(),
            "the staged file was left behind"
        );
    }

    #[test]
    fn refuses_a_binary_whose_checksum_does_not_match() {
        let sums = format!(
            "{}  {}\n",
            sha256_of(b"what we were promised"),
            asset_name()
        );
        let releases = Releases::serving(vec![
            ("checksums.txt".into(), sums.into_bytes()),
            (asset_name(), b"something else entirely".to_vec()),
        ]);

        let exe = staged_binary("tampered", b"the old binary");
        let err = replace_self(&releases.source(), &exe, "v1.4.0").unwrap_err();

        assert!(err.contains(&asset_name()), "{err}");
        // The binary we are running must survive a release we could not trust.
        assert_eq!(std::fs::read(&exe).unwrap(), b"the old binary");
    }

    #[test]
    fn refuses_a_release_that_has_no_binary_for_us() {
        let releases = Releases::serving(vec![(
            "checksums.txt".into(),
            b"aaa  sentrypanel_plan9_arm64\n".to_vec(),
        )]);

        let exe = staged_binary("missing", b"the old binary");
        let err = replace_self(&releases.source(), &exe, "v1.4.0").unwrap_err();

        assert!(err.contains("v1.4.0"), "{err}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"the old binary");
    }

    #[test]
    fn spells_a_resolved_windows_path_the_ordinary_way() {
        assert_eq!(
            plain(PathBuf::from(r"\\?\C:\Users\me\sentrypanel.exe")),
            PathBuf::from(r"C:\Users\me\sentrypanel.exe")
        );
        assert_eq!(
            plain(PathBuf::from(r"\\?\UNC\server\share\a.exe")),
            PathBuf::from(r"\\?\UNC\server\share\a.exe")
        );
        assert_eq!(
            plain(PathBuf::from("/usr/local/bin/sentrypanel")),
            PathBuf::from("/usr/local/bin/sentrypanel")
        );
    }

    #[test]
    fn reads_a_version_tag() {
        assert_eq!(parse_version("v1.2.3"), Some(Semver(1, 2, 3)));
        assert_eq!(parse_version("1.2.3"), Some(Semver(1, 2, 3)));
        assert_eq!(parse_version(" v0.0.1 "), Some(Semver(0, 0, 1)));
        // A suffix does not take part in the number.
        assert_eq!(parse_version("v1.2.3-rc1"), Some(Semver(1, 2, 3)));
    }

    #[test]
    fn refuses_what_is_not_a_version() {
        assert_eq!(parse_version("built from source"), None);
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3.4"), None);
        assert_eq!(parse_version("v1.2.x"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn compares_versions_by_number_not_by_text() {
        assert!(is_newer("v1.2.3", "v1.2.4"));
        assert!(is_newer("v1.9.0", "v1.10.0"), "9 is not newer than 10");
        assert!(is_newer("v0.9.9", "v1.0.0"));
        assert!(!is_newer("v1.2.3", "v1.2.3"));
        assert!(!is_newer("v2.0.0", "v1.0.0"));
    }

    #[test]
    fn counts_a_build_without_a_version_as_outdated_but_never_marks_it() {
        // Nothing to compare with: the mark stays out rather than lights up.
        assert!(!is_newer("built from source", "v1.2.3"));
        assert!(!is_newer("v1.2.3", "not a tag"));
    }

    #[test]
    fn finds_the_checksum_of_our_asset() {
        let sums = "aaa  sentrypanel_linux_amd64\nbbb  sentrypanel_darwin_arm64\nccc *sentrypanel_windows_amd64.exe\n";
        assert_eq!(
            checksum_for(sums, "sentrypanel_darwin_arm64").as_deref(),
            Some("bbb")
        );
        // The sha256sum format marks a binary file with a star.
        assert_eq!(
            checksum_for(sums, "sentrypanel_windows_amd64.exe").as_deref(),
            Some("ccc")
        );
        assert_eq!(checksum_for(sums, "sentrypanel_plan9_arm64"), None);
    }

    #[test]
    fn does_not_mistake_one_asset_for_another() {
        let sums = "aaa  sentrypanel_linux_amd64\nbbb  sentrypanel_linux_amd64.exe\n";
        assert_eq!(
            checksum_for(sums, "sentrypanel_linux_amd64").as_deref(),
            Some("aaa")
        );
        assert_eq!(
            checksum_for(sums, "sentrypanel_linux_amd64.exe").as_deref(),
            Some("bbb")
        );
    }

    #[test]
    fn names_the_asset_the_way_the_release_does() {
        let name = asset_name();
        assert!(name.starts_with("sentrypanel_"), "{name}");
        // The names install.sh and install.ps1 download.
        assert!(
            !name.contains("macos") && !name.contains("x86_64") && !name.contains("aarch64"),
            "{name}"
        );
        if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            assert_eq!(name, "sentrypanel_darwin_arm64");
        }
    }
}
