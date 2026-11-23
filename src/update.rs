//! Self-update: find the latest GitHub release, download its installer and
//! prove it's genuine before anything runs it.
//!
//! Trust chain:
//! 1. The release workflow writes `SHA256SUMS.txt` (hash of every file) and
//!    signs it with an Ed25519 key kept as a GitHub Actions secret, producing
//!    `SHA256SUMS.txt.sig`.
//! 2. This module checks that signature against the public key built into the
//!    app (`update-key.pub`), then checks the downloaded installer against the
//!    signed hash.
//! 3. Only a strictly newer version is accepted, so an old (possibly
//!    vulnerable) release can't be replayed as an "update".
//!
//! Anything that fails a check is discarded and nothing is installed.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REPO: &str = "Devzinh/dns-switcher";
pub const SUMS_NAME: &str = "SHA256SUMS.txt";
pub const SIG_NAME: &str = "SHA256SUMS.txt.sig";
const PUBLIC_KEY_HEX: &str = include_str!("../update-key.pub");
const MAX_INSTALLER: u64 = 64 * 1024 * 1024;
const MAX_SMALL: u64 = 1024 * 1024;

pub type Version = (u64, u64, u64);

pub fn current_version() -> Version {
    parse_version(env!("CARGO_PKG_VERSION")).expect("crate version is x.y.z")
}

/// `"v1.2.3"` or `"1.2.3"`; pre-release or build suffixes are rejected.
pub fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim().strip_prefix('v').unwrap_or(s.trim());
    let mut it = s.split('.').map(|p| p.parse::<u64>().ok());
    let v = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(v)
}

pub fn fmt_version(v: Version) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub installer_name: String,
    pub installer_url: String,
    pub sums_url: String,
    pub sig_url: String,
}

/// Picks the installer, hash list and signature out of a GitHub
/// "latest release" API response. `None` if anything is missing.
pub fn parse_release(json: &serde_json::Value) -> Option<Release> {
    if json["draft"].as_bool() == Some(true) || json["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let version = parse_version(json["tag_name"].as_str()?)?;
    let installer_name = format!("DNS-Switcher-Setup-{}.exe", fmt_version(version));
    let assets = json["assets"].as_array()?;
    let url_of = |name: &str| {
        assets
            .iter()
            .find(|a| a["name"].as_str() == Some(name))
            .and_then(|a| a["browser_download_url"].as_str())
            .map(str::to_string)
    };
    Some(Release {
        installer_url: url_of(&installer_name)?,
        sums_url: url_of(SUMS_NAME)?,
        sig_url: url_of(SIG_NAME)?,
        installer_name,
        version,
    })
}

/// Checks the detached Ed25519 signature (hex) over the hash list.
pub fn verify_signature(sums: &[u8], sig_hex: &str, public_key_hex: &str) -> Result<(), String> {
    use ed25519_dalek::{Signature, VerifyingKey};
    let key: [u8; 32] = decode_hex(public_key_hex)?.try_into().map_err(|_| "bad public key length")?;
    let sig: [u8; 64] = decode_hex(sig_hex)?.try_into().map_err(|_| "bad signature length")?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| "invalid public key")?;
    key.verify_strict(sums, &Signature::from_bytes(&sig))
        .map_err(|_| "update signature does not match".to_string())
}

/// Hash recorded for `file` in a `sha256sum`-style list (`<hex>  <name>`).
pub fn hash_for(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, name) = l.trim().split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == file && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return Err("bad hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "bad hex".to_string()))
        .collect()
}

/// Where to look for releases and which key to trust. Defaults to the
/// official repository and the built-in key; `Overrides` exist only for
/// end-to-end testing and are read by the service from an admin-only
/// registry key.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub api_url: Option<String>,
    pub public_key_hex: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .https_only(false) // checked per URL in `get`
        .user_agent(concat!("DNS-Switcher-Updater/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// HTTPS only, except plain HTTP to this machine (used by the test server).
fn allowed(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")
}

fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    if !allowed(url) {
        return Err(format!("refusing non-HTTPS URL {url}"));
    }
    let mut resp = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > limit {
        return Err(format!("{url}: response too large"));
    }
    Ok(buf)
}

/// Latest published release, if it carries a signed installer.
pub fn latest(o: &Overrides) -> Result<Option<Release>, String> {
    let url = o
        .api_url
        .clone()
        .unwrap_or_else(|| format!("https://api.github.com/repos/{REPO}/releases/latest"));
    let body = get(&agent(), &url, MAX_SMALL)?;
    let json: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    Ok(parse_release(&json))
}

/// Downloads the installer and verifies signature and hash. Returns the path
/// of the verified installer inside `dir`.
pub fn download_verified(r: &Release, dir: &Path, o: &Overrides) -> Result<PathBuf, String> {
    if r.version <= current_version() {
        return Err("not newer than the installed version".into());
    }
    let agent = agent();
    let sums = get(&agent, &r.sums_url, MAX_SMALL)?;
    let sig = get(&agent, &r.sig_url, MAX_SMALL)?;
    let key = o.public_key_hex.as_deref().unwrap_or(PUBLIC_KEY_HEX);
    verify_signature(&sums, &String::from_utf8_lossy(&sig), key)?;
    let sums = String::from_utf8_lossy(&sums);
    let expected = hash_for(&sums, &r.installer_name).ok_or("installer missing from the signed hash list")?;
    let installer = get(&agent, &r.installer_url, MAX_INSTALLER)?;
    if sha256_hex(&installer) != expected {
        return Err("installer does not match the signed hash".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(&r.installer_name);
    std::fs::write(&path, &installer).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key() -> (SigningKey, String) {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let pk: String = sk.verifying_key().to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        (sk, pk)
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("1.10.0"), Some((1, 10, 0)));
        assert!(parse_version("0.1.2-beta").is_none());
        assert!(parse_version("0.1").is_none());
        assert!(parse_version("0.1.2.3").is_none());
        assert!((0, 1, 10) > (0, 1, 9));
        assert!(parse_version(include_str!("../update-key.pub")).is_none());
    }

    #[test]
    fn signature_accepts_only_exact_content() {
        let (sk, pk) = key();
        let sums = b"abc  DNS-Switcher-Setup-0.1.3.exe\n";
        let sig = hex(&sk.sign(sums).to_bytes());
        assert!(verify_signature(sums, &sig, &pk).is_ok());
        assert!(verify_signature(b"abd  DNS-Switcher-Setup-0.1.3.exe\n", &sig, &pk).is_err());
        let (_, other_pk) = (0, hex(&SigningKey::from_bytes(&[9u8; 32]).verifying_key().to_bytes()));
        assert!(verify_signature(sums, &sig, &other_pk).is_err());
        assert!(verify_signature(sums, "zz", &pk).is_err());
    }

    #[test]
    fn built_in_key_is_valid() {
        let key = decode_hex(PUBLIC_KEY_HEX).unwrap();
        assert_eq!(key.len(), 32);
        assert!(ed25519_dalek::VerifyingKey::from_bytes(&key.try_into().unwrap()).is_ok());
    }

    #[test]
    fn hash_list() {
        let h = "a".repeat(64);
        let sums = format!("{h}  DNS-Switcher-Setup-0.1.3.exe\r\n{}  dns_switcher.exe\n", "b".repeat(64));
        assert_eq!(hash_for(&sums, "DNS-Switcher-Setup-0.1.3.exe"), Some(h));
        assert_eq!(hash_for(&sums, "other.exe"), None);
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn release_parsing() {
        let json = serde_json::json!({
            "tag_name": "v0.1.3", "draft": false, "prerelease": false,
            "assets": [
                {"name": "DNS-Switcher-Setup-0.1.3.exe", "browser_download_url": "https://x/setup"},
                {"name": "SHA256SUMS.txt", "browser_download_url": "https://x/sums"},
                {"name": "SHA256SUMS.txt.sig", "browser_download_url": "https://x/sig"}
            ]
        });
        let r = parse_release(&json).unwrap();
        assert_eq!(r.version, (0, 1, 3));
        assert_eq!(r.installer_url, "https://x/setup");
        let mut unsigned = json.clone();
        unsigned["assets"].as_array_mut().unwrap().pop();
        assert!(parse_release(&unsigned).is_none());
        let mut pre = json;
        pre["prerelease"] = true.into();
        assert!(parse_release(&pre).is_none());
    }

    #[test]
    fn only_https_or_local() {
        assert!(allowed("https://api.github.com/x"));
        assert!(allowed("http://127.0.0.1:8080/x"));
        assert!(!allowed("http://github.com/x"));
        assert!(!allowed("file:///c:/x"));
    }
}
