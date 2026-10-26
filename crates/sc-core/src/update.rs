//! Looking for a newer SimpleClip on GitHub. Shared by the app (the "Update
//! available" button) and the daemon (the card shown at start-up). Network access
//! goes through `curl`, so there is no HTTP library to carry around.

use std::process::Command;

const DOWNLOAD_PREFIX: &str =
    "https://github.com/whoevenisxeno/simpleclip-linux/releases/download/";
const API: &str = "https://api.github.com/repos/whoevenisxeno/simpleclip-linux/releases/latest";

/// One file attached to a release.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub url: String,
    pub size: u64,
    /// SHA-256 as GitHub reports it (lowercase hex), when it does.
    pub sha256: Option<String>,
}

/// A release newer than the running version. Updating never runs an installer:
/// the release carries a tarball with the three programs, which replace the
/// installed ones in place.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub package: Asset,
}

/// `4.1.0` / `v4.1.0` as numbers.
fn parse(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|p| {
            p.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

/// Is `remote` a higher version than `local`?
pub fn is_newer(remote: &str, local: &str) -> bool {
    let (mut r, mut l) = (parse(remote), parse(local));
    let n = r.len().max(l.len()).max(3);
    r.resize(n, 0);
    l.resize(n, 0);
    r > l
}

/// How many major versions `remote` is ahead of `local` (0 when it is not ahead).
pub fn major_gap(remote: &str, local: &str) -> u64 {
    let first = |v: &str| parse(v).first().copied().unwrap_or(0);
    first(remote).saturating_sub(first(local))
}

/// Fetch a URL with curl.
pub fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "--max-time", "30", "-H", "User-Agent: SimpleClip"])
        .args(args);
    let out = cmd
        .output()
        .map_err(|e| format!("couldn't run curl: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "couldn't reach GitHub ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Read the latest release's JSON: `None` when it isn't newer than `current`.
pub fn parse_release(json: &[u8], current: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| "unexpected reply from GitHub".to_string())?;
    let tag = v["tag_name"].as_str().ok_or("no release found")?;
    if !is_newer(tag, current) {
        return Ok(None);
    }
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let a = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .is_some_and(|n| n.ends_with("-linux-x86_64.tar.gz"))
        })
        .ok_or("this release has no update package attached")?;
    let url = a["browser_download_url"]
        .as_str()
        .ok_or("an update file has no download link")?;
    // Only ever download from this project's own releases.
    if !url.starts_with(DOWNLOAD_PREFIX) {
        return Err("an update file is not hosted where SimpleClip expects".to_string());
    }
    Ok(Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"].as_str().unwrap_or("").to_string(),
        package: Asset {
            url: url.to_string(),
            size: a["size"].as_u64().unwrap_or(0),
            sha256: a["digest"]
                .as_str()
                .and_then(|d| d.strip_prefix("sha256:"))
                .map(|h| h.to_ascii_lowercase()),
        },
    }))
}

/// Ask GitHub for the latest release; `Ok(None)` means `current` is up to date.
pub fn fetch_latest(current: &str) -> Result<Option<Release>, String> {
    curl(&[API]).and_then(|j| parse_release(&j, current))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v4.1.0", "4.0.0"));
        assert!(is_newer("4.0.1", "4.0.0"));
        assert!(is_newer("5.0.0", "4.9.9"));
        assert!(is_newer("4.10.0", "4.9.0"));
        assert!(!is_newer("4.0.0", "4.0.0"));
        assert!(!is_newer("v3.9.9", "4.0.0"));
        assert!(is_newer("4.0.0.1", "4.0.0"));
    }

    #[test]
    fn major_gap_counts_major_versions() {
        assert_eq!(major_gap("v9.1.0", "4.7.2"), 5);
        assert_eq!(major_gap("4.9.9", "4.0.0"), 0);
        assert_eq!(major_gap("3.0.0", "4.0.0"), 0);
    }

    #[test]
    fn release_json_is_read() {
        let base = "https://github.com/whoevenisxeno/simpleclip-linux/releases/download/v99.0.0/";
        let json = format!(
            r###"{{"tag_name":"v99.0.0","body":"## 99.0.0\n- x","assets":[
            {{"name":"notes.txt","browser_download_url":"{base}notes.txt","size":9}},
            {{"name":"simpleclip-99.0.0-linux-x86_64.tar.gz","browser_download_url":"{base}simpleclip-99.0.0-linux-x86_64.tar.gz","size":42,"digest":"sha256:ABCDEF"}}]}}"###
        );
        let r = parse_release(json.as_bytes(), "4.0.0").unwrap().unwrap();
        assert_eq!(r.version, "99.0.0");
        assert!(r.package.url.ends_with("-linux-x86_64.tar.gz"));
        assert_eq!(r.package.sha256.as_deref(), Some("abcdef"));
        assert_eq!(r.package.size, 42);
        // A download from anywhere else is refused.
        let evil = br###"{"tag_name":"v99.0.0","assets":[{"name":"x-linux-x86_64.tar.gz","browser_download_url":"https://evil.example/x-linux-x86_64.tar.gz","size":1}]}"###;
        assert!(parse_release(evil, "4.0.0").is_err());
        // A release with no package attached can not be applied in place.
        let none = br###"{"tag_name":"v99.0.0","assets":[{"name":"notes.txt","browser_download_url":"https://github.com/whoevenisxeno/simpleclip-linux/releases/download/v99.0.0/notes.txt","size":1}]}"###;
        assert!(parse_release(none, "4.0.0").is_err());
        let old = br#"{"tag_name":"v0.0.1","assets":[]}"#;
        assert!(parse_release(old, "4.0.0").unwrap().is_none());
    }
}
