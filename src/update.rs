//! Checks GitHub for a newer release in the background.
//!
//! One small HTTPS request to the public GitHub API on startup (no account,
//! no identifiers beyond the app name and version in the User-Agent). The UI
//! polls [`Updater::status`] and offers a download button when a newer
//! version exists.

use std::sync::{Arc, Mutex};
use std::time::Duration;

const REPO: &str = "jonbp/arthurs-universe-playground";
/// Only links under this prefix are ever opened in the browser.
const TRUSTED_PREFIX: &str = "https://github.com/jonbp/arthurs-universe-playground/";

/// File-name ending of the release asset built for this platform (see release.yml).
const ASSET_SUFFIX: Option<&str> = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    Some("windows-x64.zip")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("macos-arm64.dmg")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("linux-x86_64.tar.gz")
} else {
    None
};

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Version without the leading "v", e.g. "1.2.0".
    pub version: String,
    /// The release page on GitHub.
    pub page_url: String,
    /// Direct download for this platform, when the release has one.
    pub asset_url: Option<String>,
}

impl Release {
    /// What the download button opens: this platform's file, else the release page.
    pub fn download_url(&self) -> &str {
        self.asset_url.as_deref().unwrap_or(&self.page_url)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Update checks are switched off (tests, screenshots, `--no-update-check`).
    Disabled,
    Checking,
    UpToDate,
    Available(Release),
    /// Offline, rate-limited, ... Never shown as an error dialog; the help window mentions it.
    Failed(String),
}

pub struct Updater {
    current: String,
    status: Arc<Mutex<Status>>,
    /// The "update available" banner was closed for this session.
    pub dismissed: bool,
}

impl Updater {
    pub fn disabled() -> Updater {
        Updater { current: env!("CARGO_PKG_VERSION").to_string(), status: Arc::new(Mutex::new(Status::Disabled)), dismissed: false }
    }

    /// Start a background check against `current` (normally the crate version).
    pub fn start(current: &str) -> Updater {
        let mut u = Updater { current: current.to_string(), status: Arc::new(Mutex::new(Status::Disabled)), dismissed: false };
        u.check();
        u
    }

    /// (Re)run the check unless one is already in flight.
    pub fn check(&mut self) {
        {
            let mut s = self.status.lock().unwrap();
            if *s == Status::Checking {
                return;
            }
            *s = Status::Checking;
        }
        self.dismissed = false;
        let status = self.status.clone();
        let current = self.current.clone();
        let spawned = std::thread::Builder::new().name("update-check".into()).spawn(move || {
            let result = match fetch_latest() {
                Ok(json) => match parse_release(&json) {
                    Some(r) if is_newer(&r.version, &current) => Status::Available(r),
                    Some(_) => Status::UpToDate,
                    None => Status::Failed("unexpected reply from GitHub".into()),
                },
                Err(e) => Status::Failed(e),
            };
            *status.lock().unwrap() = result;
        });
        if spawned.is_err() {
            *self.status.lock().unwrap() = Status::Failed("could not start the update check".into());
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn current_version(&self) -> &str {
        &self.current
    }
}

fn fetch_latest() -> Result<serde_json::Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(12))).build().into();
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut resp = agent
        .get(&url)
        .header("User-Agent", concat!("arthurs-universe-playground/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    resp.body_mut().read_json::<serde_json::Value>().map_err(|e| e.to_string())
}

/// Pull the version and links out of GitHub's "latest release" JSON.
fn parse_release(json: &serde_json::Value) -> Option<Release> {
    let tag = json.get("tag_name")?.as_str()?;
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    parse_version(&version)?;
    let trusted = |u: &str| u.starts_with(TRUSTED_PREFIX);
    let page_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .filter(|u| trusted(u))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{TRUSTED_PREFIX}releases/latest"));
    let asset_url = ASSET_SUFFIX.and_then(|suffix| {
        json.get("assets")?.as_array()?.iter().find_map(|a| {
            let name = a.get("name")?.as_str()?;
            let url = a.get("browser_download_url")?.as_str()?;
            (name.ends_with(suffix) && trusted(url)).then(|| url.to_string())
        })
    });
    Some(Release { version, page_url, asset_url })
}

/// "1.2.3" (optionally with a "-beta"/"+build" tail, which is ignored) -> [1, 2, 3].
fn parse_version(v: &str) -> Option<Vec<u64>> {
    let core = v.trim().trim_start_matches(['v', 'V']);
    let core = core.split(['-', '+']).next()?;
    let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

/// Is version `a` strictly newer than `b`? Unparseable versions are never "newer".
pub fn is_newer(a: &str, b: &str) -> bool {
    let (Some(mut a), Some(mut b)) = (parse_version(a), parse_version(b)) else { return false };
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering() {
        assert!(is_newer("1.2.0", "1.1.0"));
        assert!(is_newer("v1.10.0", "1.9.3"));
        assert!(is_newer("2.0", "1.99.99"));
        assert!(is_newer("1.1.1", "1.1"));
        assert!(!is_newer("1.1.0", "1.1.0"));
        assert!(!is_newer("1.1", "1.1.0"));
        assert!(!is_newer("1.0.9", "1.1.0"));
        assert!(!is_newer("garbage", "1.1.0"));
        assert!(!is_newer("1.2.0-beta", "1.2.0"));
    }

    #[test]
    fn parses_github_release_json() {
        let json = serde_json::json!({
            "tag_name": "v9.4.1",
            "html_url": "https://github.com/jonbp/arthurs-universe-playground/releases/tag/v9.4.1",
            "assets": [
                { "name": "ArthursUniversePlayground-9.4.1-windows-x64.zip",
                  "browser_download_url": "https://github.com/jonbp/arthurs-universe-playground/releases/download/v9.4.1/ArthursUniversePlayground-9.4.1-windows-x64.zip" },
                { "name": "ArthursUniversePlayground-9.4.1-macos-arm64.dmg",
                  "browser_download_url": "https://github.com/jonbp/arthurs-universe-playground/releases/download/v9.4.1/ArthursUniversePlayground-9.4.1-macos-arm64.dmg" },
                { "name": "ArthursUniversePlayground-9.4.1-linux-x86_64.tar.gz",
                  "browser_download_url": "https://github.com/jonbp/arthurs-universe-playground/releases/download/v9.4.1/ArthursUniversePlayground-9.4.1-linux-x86_64.tar.gz" }
            ]
        });
        let r = parse_release(&json).expect("release parsed");
        assert_eq!(r.version, "9.4.1");
        assert!(r.page_url.ends_with("/releases/tag/v9.4.1"));
        if let Some(suffix) = ASSET_SUFFIX {
            assert!(r.asset_url.as_deref().unwrap().ends_with(suffix));
            assert!(r.download_url().ends_with(suffix));
        } else {
            assert_eq!(r.download_url(), r.page_url);
        }
    }

    #[test]
    fn untrusted_links_are_dropped() {
        let json = serde_json::json!({
            "tag_name": "v9.0.0",
            "html_url": "https://evil.example/releases",
            "assets": [
                { "name": "x-windows-x64.zip", "browser_download_url": "https://evil.example/x-windows-x64.zip" },
                { "name": "x-macos-arm64.dmg", "browser_download_url": "https://evil.example/x-macos-arm64.dmg" },
                { "name": "x-linux-x86_64.tar.gz", "browser_download_url": "https://evil.example/x-linux-x86_64.tar.gz" }
            ]
        });
        let r = parse_release(&json).unwrap();
        assert!(r.asset_url.is_none());
        assert!(r.download_url().starts_with(TRUSTED_PREFIX));
    }

    #[test]
    fn missing_tag_is_rejected() {
        assert!(parse_release(&serde_json::json!({ "message": "Not Found" })).is_none());
        assert!(parse_release(&serde_json::json!({ "tag_name": "nightly" })).is_none());
    }
}
