use super::{CheckError, CheckResult};
use crate::{
    MAX_MANIFEST, Platform, Result, Selection, Staged, finish_download, new_download, pin,
    select_manifest, stream_body,
};
use reqwest::{Client, Response, StatusCode, Url, header, redirect::Policy};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path, time::Duration};

const GITHUB: &str = "https://api.github.com/repos/joshmarkovic/threadline/";
const DOWNLOADS: &str = "https://github.com/joshmarkovic/threadline/releases/download/";
const MANIFEST_NAME: &str = "threadline-windows-x64.json";
const PRIVATE_MANIFEST: &str = "threadline-private-windows-x64.json";
const PRIVATE_SIGNATURE: &str = "threadline-private-windows-x64.json.sig";
const PAGE_SIZE: usize = 20;
const MAX_RELEASE: u64 = 1024 * 1024;
const INVALID: &str = "GitHub returned an incomplete or ambiguous Windows release.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Trust {
    Authenticode([u8; 32]),
    Private([u8; 65]),
}

pub(super) fn private_key(hex: &str) -> Result<[u8; 65]> {
    let coordinates = hex
        .strip_prefix("04")
        .filter(|c| c.len() == 128 && c.is_ascii())
        .ok_or("The embedded private update key is invalid.")?;
    let mut key = [0; 65];
    key[0] = 4;
    key[1..33].copy_from_slice(&pin(&coordinates[..64])?);
    key[33..].copy_from_slice(&pin(&coordinates[64..])?);
    Ok(key)
}

pub(super) fn private_build(version: &Version) -> Option<u32> {
    let build = version
        .pre
        .as_str()
        .strip_prefix("local.")?
        .parse::<u32>()
        .ok()?;
    (build != 0 && version.build.is_empty() && version.pre.as_str() == format!("local.{build}"))
        .then_some(build)
}

pub(crate) struct Installed {
    pub version: Version,
    pub build: u32,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

impl Release {
    fn asset(&self, leaf: &str, expected_url: &Url, max_size: u64) -> Result<(&Asset, [u8; 32])> {
        let mut matches = self.assets.iter().filter(|a| a.name == leaf);
        let a = matches.next().ok_or(INVALID)?;
        if matches.next().is_some()
            || a.state != "uploaded"
            || a.id == 0
            || self.assets.iter().filter(|other| other.id == a.id).count() != 1
            || a.browser_download_url != expected_url.as_str()
            || a.size == 0
            || a.size > max_size
        {
            return Err(INVALID);
        }
        let hash = a
            .digest
            .as_deref()
            .and_then(|s| s.strip_prefix("sha256:"))
            .ok_or(INVALID)?;
        Ok((a, pin(hash)?))
    }
}

pub(super) struct Github {
    api: Url,
    client: Client,
    trust: Trust,
}

fn allowed_cdn(to: &Url) -> bool {
    to.scheme() == "https"
        && to.host_str() == Some("release-assets.githubusercontent.com")
        && to.port_or_known_default() == Some(443)
        && to.username().is_empty()
        && to.password().is_none()
        && to.fragment().is_none()
}

impl Github {
    pub(super) fn new(trust: Trust) -> Result<Self> {
        Ok(Self {
            api: Url::parse(GITHUB).map_err(|_| INVALID)?,
            client: Client::builder()
                .redirect(Policy::none())
                .user_agent("Threadline Windows updates")
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(120))
                .build()
                .map_err(|_| "Could not initialize HTTPS for Windows updates.")?,
            trust,
        })
    }

    async fn request(
        &self,
        route: &str,
        accept: &'static str,
        token: Option<&str>,
    ) -> Result<Response> {
        // Only fixed routes with decimal page numbers or asset IDs enter this request.
        let url = if route.is_empty() {
            Url::parse(self.api.as_str().trim_end_matches('/'))
        } else {
            self.api.join(route)
        }
        .map_err(|_| INVALID)?;
        if url.origin() != self.api.origin()
            || (!route.is_empty() && !url.path().starts_with(self.api.path()))
        {
            return Err(INVALID);
        }
        let mut get = self
            .client
            .get(url)
            .header(header::ACCEPT, accept)
            .header("X-GitHub-Api-Version", "2026-03-10");
        if let Some(token) = token {
            get = get.bearer_auth(token);
        }
        get.send()
            .await
            .map_err(|_| "Could not reach GitHub to check for Windows updates.")
    }

    fn cdn_download(
        &self,
        location: Option<&header::HeaderValue>,
    ) -> Result<reqwest::RequestBuilder> {
        let url = location
            .and_then(|v| v.to_str().ok())
            .and_then(|s| Url::parse(s).ok())
            .filter(allowed_cdn)
            .ok_or("GitHub redirected the release asset outside its approved download host.")?;
        // The CDN receives only its own short-lived URL.
        Ok(self.client.get(url))
    }

    async fn asset_bytes(
        &self,
        asset: &Asset,
        out: &mut (dyn Write + Send),
        token: Option<&str>,
    ) -> CheckResult<()> {
        let mut response = self
            .request(
                &format!("releases/assets/{}", asset.id),
                "application/octet-stream",
                token,
            )
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            self.check_visibility(token).await?;
            return Err(CheckError::Other(
                "The release asset is no longer available. Check for updates again.",
            ));
        }
        Self::api_status(response.status())?;
        for _ in 0..3 {
            if !matches!(
                response.status(),
                StatusCode::FOUND | StatusCode::TEMPORARY_REDIRECT
            ) {
                break;
            }
            response = self
                .cdn_download(response.headers().get(header::LOCATION))?
                .send()
                .await
                .map_err(|_| "Could not download the selected Windows update from GitHub.")?;
        }
        if stream_body(response, out, asset.size).await? != asset.size {
            return Err(CheckError::Other(
                "GitHub returned an incomplete Windows update asset.",
            ));
        }
        Ok(())
    }

    fn api_status(status: StatusCode) -> CheckResult<()> {
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::NOT_FOUND => {
                Err(CheckError::ReleaseAccessRequired)
            }
            StatusCode::FORBIDDEN => Err(CheckError::Other(
                "GitHub refused this update request. Try again later.",
            )),
            StatusCode::TOO_MANY_REQUESTS => Err(CheckError::Other(
                "GitHub is limiting update requests. Try again later.",
            )),
            _ => Ok(()),
        }
    }

    async fn json<T: serde::de::DeserializeOwned>(response: Response) -> CheckResult<T> {
        Self::api_status(response.status())?;
        let mut raw = Vec::new();
        stream_body(response, &mut raw, MAX_RELEASE).await?;
        serde_json::from_slice(&raw).map_err(|_| CheckError::Other(INVALID))
    }

    async fn check_visibility(&self, token: Option<&str>) -> CheckResult<()> {
        // Repository metadata can be public to the caller while releases are inaccessible.
        Self::json::<Vec<Release>>(
            self.request("releases?per_page=1", "application/vnd.github+json", token)
                .await?,
        )
        .await?;
        Ok(())
    }

    async fn release(&self, token: Option<&str>) -> CheckResult<Option<(Version, Release)>> {
        let accept = "application/vnd.github+json";
        if matches!(self.trust, Trust::Authenticode(_)) {
            let response = self.request("releases/latest", accept, token).await?;
            if response.status() == StatusCode::NOT_FOUND {
                self.check_visibility(token).await?;
                return Ok(None);
            }
            let release: Release = Self::json(response).await?;
            let version = release
                .tag_name
                .strip_prefix('v')
                .and_then(|v| Version::parse(v).ok())
                .ok_or(INVALID)?;
            return Ok(
                (!release.draft && !release.prerelease && version.pre.is_empty())
                    .then_some((version, release)),
            );
        }

        let mut latest: Option<(Version, Release)> = None;
        for page in 1..=100 {
            let route = format!("releases?per_page={PAGE_SIZE}&page={page}");
            let releases: Vec<Release> =
                Self::json(self.request(&route, accept, token).await?).await?;
            let last = releases.len() < PAGE_SIZE;
            for release in releases {
                if !release.prerelease || release.draft {
                    continue;
                }
                let Some(version) = release
                    .tag_name
                    .strip_prefix('v')
                    .and_then(|v| Version::parse(v).ok())
                else {
                    continue;
                };
                if private_build(&version).is_some()
                    && latest.as_ref().is_none_or(|(v, _)| &version > v)
                {
                    latest = Some((version, release));
                }
            }
            if last {
                return Ok(latest);
            }
        }
        Err(CheckError::Other(
            "GitHub returned too many release pages to select an update.",
        ))
    }

    pub(super) async fn stage(
        &self,
        current: &Installed,
        host: &Platform,
        temp: &Path,
        token: Option<&str>,
    ) -> CheckResult<Option<Staged>> {
        host.check()?;
        let Some((version, release)) = self.release(token).await? else {
            return Ok(None);
        };
        if !version.cmp_precedence(&current.version).is_gt() {
            return Ok(None);
        }
        let dir = Url::parse(DOWNLOADS)
            .map_err(|_| INVALID)?
            .join(&format!("v{version}/"))
            .map_err(|_| INVALID)?;
        let (name, channel) = match self.trust {
            Trust::Authenticode(_) => (MANIFEST_NAME, "stable"),
            Trust::Private(_) => (PRIVATE_MANIFEST, "private-preview"),
        };
        let (manifest_asset, manifest_hash) =
            release.asset(name, &dir.join(name).map_err(|_| INVALID)?, MAX_MANIFEST)?;
        let selection = Selection {
            version,
            channel: channel.into(),
            manifest_hash,
        };
        let mut raw = Vec::new();
        self.asset_bytes(manifest_asset, &mut raw, token).await?;
        if let Trust::Private(key) = self.trust {
            let sig_url = dir.join(PRIVATE_SIGNATURE).map_err(|_| INVALID)?;
            let (sig_asset, sig_hash) = release.asset(PRIVATE_SIGNATURE, &sig_url, 64)?;
            let mut signature = Vec::new();
            self.asset_bytes(sig_asset, &mut signature, token).await?;
            if signature.len() != 64 || Sha256::digest(&signature).as_slice() != sig_hash {
                return Err(CheckError::Other(
                    "The private update signature is incomplete or corrupt.",
                ));
            }
            ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, key)
                .verify(&raw, &signature)
                .map_err(|_| "The private update manifest failed signature verification.")?;
        }
        let (manifest, url) = select_manifest(&dir, &raw, &selection, host)?;
        match self.trust {
            Trust::Authenticode(publisher)
                if pin(&manifest.publisher_certificate_sha256)? != publisher =>
            {
                return Err(CheckError::Other(
                    "The update manifest names a different publisher.",
                ));
            }
            Trust::Private(_) => {
                let expected_url = dir
                    .join(&format!("Threadline-Setup-{}.exe", manifest.package_build))
                    .map_err(|_| INVALID)?;
                if private_build(&selection.version) != Some(manifest.package_build)
                    || url != expected_url
                {
                    return Err(CheckError::Other(
                        "The private update does not match its release tag and installer name.",
                    ));
                }
            }
            _ => {}
        }
        if manifest.restart_protocol != Some(1)
            || manifest.package_build <= current.build.max(host.highest_build)
        {
            return Ok(None);
        }
        let leaf = url
            .path_segments()
            .and_then(|mut p| p.next_back())
            .ok_or(INVALID)?;
        let (setup, hash) = release.asset(leaf, &url, manifest.size)?;
        if setup.size != manifest.size || hash != pin(&manifest.sha256)? {
            return Err(CheckError::Other(
                "The installer asset differs from the approved Windows release manifest.",
            ));
        }
        let (dir, path, mut output) = new_download(temp)?;
        self.asset_bytes(setup, &mut output, token).await?;
        Ok(Some(finish_download(
            dir, path, output, setup.size, manifest,
        )?))
    }
}

#[cfg(test)]
mod tests;
