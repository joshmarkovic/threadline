//! Select and stage the exact Windows installer before its native publisher and setup checks.
use reqwest::{Client, Url, redirect::Policy};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_INSTALLER: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST: u64 = 64 * 1024;
pub const TARGET: &str = "x86_64-pc-windows-msvc";
const MINIMUM: &str = "Windows 11 x64 build 22000";
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug)]
pub struct Selection {
    pub version: Version,
    pub channel: String,
    pub manifest_hash: [u8; 32],
}
impl Selection {
    pub fn new(version: &str, channel: &str, manifest_hash: &str) -> Result<Self> {
        let version =
            Version::parse(version).map_err(|_| "Select a valid exact release version.")?;
        if !matches!(channel, "preview" | "stable")
            || channel == "stable" && !version.pre.is_empty()
        {
            return Err("The requested channel is unavailable for this version.");
        }
        Ok(Self {
            version,
            channel: channel.into(),
            manifest_hash: pin(manifest_hash)?,
        })
    }
}
pub fn pin(text: &str) -> Result<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("A trusted SHA-256 pin is required.");
    }
    let mut out = [0; 32];
    for (i, n) in out.iter_mut().enumerate() {
        *n = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| "Invalid SHA-256 pin.")?;
    }
    Ok(out)
}

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub channel: String,
    pub target: String,
    pub minimum_os: String,
    pub artifact_url: String,
    pub package_build: u32,
    pub current_schema: u32,
    pub oldest_supported_schema: u32,
    pub size: u64,
    pub sha256: String,
    pub publisher_certificate_sha256: String,
}
#[derive(Clone, Copy, Debug)]
pub struct Platform {
    pub native_x64: bool,
    pub workstation: bool,
    pub build: u32,
    pub graphics: bool,
    pub desktop: bool,
    pub highest_build: u32,
    pub profile_schema: Option<u32>,
}
impl Platform {
    pub fn check(&self) -> Result<()> {
        if !self.native_x64 || !self.workstation || self.build < 22000 {
            return Err("Threadline requires Windows 11 on native x64 hardware.");
        }
        if !self.desktop {
            return Err("Run setup in an interactive Windows desktop session.");
        }
        if !self.graphics {
            return Err(
                "Windows could not create a Direct3D device at feature level 10.1 or higher. Check the display driver.",
            );
        }
        Ok(())
    }
}

pub struct Source {
    base: Url,
    publisher: [u8; 32],
    client: Client,
}
impl Source {
    pub fn https(base: &str, publisher: &str) -> Result<Self> {
        let url = Url::parse(base).map_err(|_| "The configured release origin is invalid.")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || !url.path().ends_with('/')
            || !safe_segments(base)
        {
            return Err("Use the configured HTTPS release directory.");
        }
        Self::with_url(url, pin(publisher)?)
    }
    fn with_url(base: Url, publisher: [u8; 32]) -> Result<Self> {
        let first = base.clone();
        let redirects = Policy::custom(move |attempt| {
            let to = attempt.url();
            let github_signed_asset = first.host_str() == Some("github.com")
                && to.host_str() == Some("release-assets.githubusercontent.com")
                && to.port_or_known_default() == Some(443);
            if attempt.previous().len() >= 4
                || to.scheme() != "https"
                || !to.username().is_empty()
                || to.password().is_some()
                || (to.origin() != first.origin() && !github_signed_asset)
            {
                return attempt.error("Release redirect refused");
            }
            attempt.follow()
        });
        let client = Client::builder()
            .user_agent("Threadline Windows installer")
            .redirect(redirects)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| "Could not initialize HTTPS for the Windows installer.")?;
        Ok(Self {
            base,
            publisher,
            client,
        })
    }
    pub fn publisher(&self) -> [u8; 32] {
        self.publisher
    }
    fn version_dir(&self, selection: &Selection) -> Result<Url> {
        self.base
            .join(&format!("v{}/", selection.version))
            .map_err(|_| "Could not locate the selected release.")
    }
    fn select(&self, raw: &[u8], wanted: &Selection, host: &Platform) -> Result<(Manifest, Url)> {
        host.check()?;
        if raw.len() > MAX_MANIFEST as usize
            || Sha256::digest(raw).as_slice() != wanted.manifest_hash
        {
            return Err("The release manifest failed its independently supplied digest.");
        }
        let m: Manifest = serde_json::from_slice(raw)
            .map_err(|_| "The trusted manifest does not describe a signed release installer.")?;
        if m.version != wanted.version.to_string()
            || m.channel != wanted.channel
            || m.target != TARGET
            || m.minimum_os != MINIMUM
        {
            return Err(
                "The manifest does not match the exact requested version, channel and Windows target.",
            );
        }
        if m.package_build == 0 || m.package_build < host.highest_build {
            return Err("The installed profile requires the same or a newer build.");
        }
        if m.oldest_supported_schema == 0
            || m.oldest_supported_schema > m.current_schema
            || host
                .profile_schema
                .is_some_and(|s| s < m.oldest_supported_schema || s > m.current_schema)
        {
            return Err("This installer is incompatible with the retained profile schema.");
        }
        if m.size == 0
            || m.size > MAX_INSTALLER
            || pin(&m.sha256).is_err()
            || pin(&m.publisher_certificate_sha256)? != self.publisher
        {
            return Err("The approved release size, digest or publisher is invalid.");
        }
        let url = Url::parse(&m.artifact_url).map_err(|_| "The installer asset URL is invalid.")?;
        let dir = self.version_dir(wanted)?;
        let leaf = url.path().strip_prefix(dir.path()).unwrap_or_default();
        if !safe_segments(&m.artifact_url)
            || url.origin() != dir.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || leaf.is_empty()
            || leaf.contains('/')
            || !leaf.ends_with(".exe")
            || !safe_filename(leaf)
            || !leaf
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        {
            return Err("The installer asset is outside the configured release directory.");
        }
        Ok((m, url))
    }
    pub async fn stage(
        &self,
        wanted: &Selection,
        host: &Platform,
        offline: Option<&Path>,
    ) -> Result<Staged> {
        self.stage_in(wanted, host, offline, &std::env::temp_dir())
            .await
    }
    async fn stage_in(
        &self,
        wanted: &Selection,
        host: &Platform,
        offline: Option<&Path>,
        temp: &Path,
    ) -> Result<Staged> {
        host.check()?;
        let manifest_url = self
            .version_dir(wanted)?
            .join("threadline-windows-x64.json")
            .map_err(|_| "Invalid manifest location.")?;
        let mut raw = Vec::new();
        match offline {
            Some(dir) => read_local(
                &dir.join("threadline-windows-x64.json"),
                &mut raw,
                MAX_MANIFEST,
            )?,
            None => self.fetch(manifest_url, &mut raw, MAX_MANIFEST).await?,
        };
        let (manifest, asset) = self.select(&raw, wanted, host)?;
        let directory = tempfile::Builder::new()
            .prefix("Threadline-setup-")
            .tempdir_in(temp)
            .map_err(|_| "Could not create a private temporary staging directory.")?;
        let path = directory.path().join("ThreadlineSetup.exe");
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "Could not create the staged installer.")?;
        let length = match offline {
            Some(dir) => read_local(
                &dir.join(
                    asset
                        .path_segments()
                        .and_then(|mut x| x.next_back())
                        .ok_or("Missing installer name.")?,
                ),
                &mut output,
                manifest.size,
            )?,
            None => self.fetch(asset, &mut output, manifest.size).await?,
        };
        output
            .sync_all()
            .map_err(|_| "Could not flush the complete installer. Check available disk space.")?;
        drop(output);
        if length != manifest.size {
            return Err("The installer download was interrupted or had the wrong size.");
        }
        let guard = lock_installer(&path)?;
        verify_bytes(&guard, manifest.size, &pin(&manifest.sha256)?)?;
        Ok(Staged {
            manifest,
            path,
            _directory: directory,
            _guard: guard,
        })
    }
    async fn fetch(&self, url: Url, out: &mut dyn Write, limit: u64) -> Result<u64> {
        let mut response = self.client.get(url).send().await.map_err(|_| {
            "Could not reach the selected release. Check network access or use a verified offline copy."
        })?;
        if response.status() != reqwest::StatusCode::OK {
            return Err("The selected release or installer is unavailable.");
        }
        if response.content_length().is_some_and(|n| n > limit) {
            return Err("The download exceeded its allowed size.");
        }
        let mut count = 0;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "The download was interrupted. Rerun setup to start a fresh download.")?
        {
            count += chunk.len() as u64;
            if count > limit {
                return Err("The download exceeded its allowed size.");
            }
            out.write_all(&chunk)
                .map_err(|_| "Could not write the staged download. Check available disk space.")?;
        }
        Ok(count)
    }
}
fn safe_segments(raw: &str) -> bool {
    !raw.contains(['\\', '%']) && !raw.split('/').any(|p| matches!(p, "." | ".."))
}
fn safe_filename(leaf: &str) -> bool {
    let stem = leaf.split('.').next().unwrap_or("").to_ascii_uppercase();
    !stem.is_empty()
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}
fn read_local(path: &Path, out: &mut dyn Write, limit: u64) -> Result<u64> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "A required offline release file is missing.")?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("The local release file has an invalid type or size.");
    }
    let file = File::open(path).map_err(|_| "Could not open the local release file.")?;
    let count = io::copy(&mut file.take(limit + 1), out)
        .map_err(|_| "Could not read or stage the complete offline release file.")?;
    if count > limit {
        return Err("The local release file exceeded its allowed size.");
    }
    Ok(count)
}
fn lock_installer(path: &Path) -> Result<File> {
    let mut options = File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    options
        .open(path)
        .map_err(|_| "Could not hold the staged installer for verification.")
}
fn verify_bytes(file: &File, size: u64, hash: &[u8; 32]) -> Result<()> {
    let mut reader = file;
    let mut digest = Sha256::new();
    let mut n = 0;
    let mut buffer = [0; 32 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| "Could not verify the complete staged installer.")?;
        if read == 0 {
            break;
        }
        n += read as u64;
        digest.update(&buffer[..read]);
    }
    if n != size || digest.finalize().as_slice() != hash {
        return Err("The staged installer failed its independent release digest.");
    }
    Ok(())
}
pub struct Staged {
    pub manifest: Manifest,
    pub path: PathBuf,
    // Release the read-only Windows handle before removing its private directory.
    _guard: File,
    _directory: tempfile::TempDir,
}
#[cfg(test)]
mod tests;
