use crate::{
    MAX_MANIFEST, Platform, Result, Selection, Source, Staged, finish_download, new_download, pin,
    stream_body,
};
use reqwest::{Client, Response, StatusCode, Url, header, redirect::Policy};
use semver::Version;
use serde::Deserialize;
use std::{io::Write, path::Path, time::Duration};

const GITHUB: &str = "https://api.github.com/repos/joshmarkovic/threadline/";
const DOWNLOADS: &str = "https://github.com/joshmarkovic/threadline/releases/download/";
const MANIFEST_NAME: &str = "threadline-windows-x64.json";
const MAX_RELEASE: u64 = 1024 * 1024;
const INVALID: &str = "GitHub returned an incomplete or ambiguous Windows release.";

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
    source: Source,
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
    pub(super) fn new(publisher: &str) -> Result<Self> {
        Ok(Self {
            api: Url::parse(GITHUB).map_err(|_| INVALID)?,
            client: Client::builder()
                .redirect(Policy::none())
                .user_agent("Threadline Windows updates")
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(120))
                .build()
                .map_err(|_| "Could not initialize HTTPS for Windows updates.")?,
            source: Source::https(DOWNLOADS, publisher)?,
        })
    }

    async fn request(
        &self,
        route: &str,
        accept: &'static str,
        token: Option<&str>,
    ) -> Result<Response> {
        // Only internal constant routes and decimal asset IDs enter this request.
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
    ) -> Result<()> {
        let mut response = self
            .request(
                &format!("releases/assets/{}", asset.id),
                "application/octet-stream",
                token,
            )
            .await?;
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
            return Err("GitHub returned an incomplete Windows update asset.");
        }
        Ok(())
    }

    pub(super) async fn stage(
        &self,
        current: &Installed,
        host: &Platform,
        temp: &Path,
        token: Option<&str>,
    ) -> Result<Option<Staged>> {
        host.check()?;
        let response = self
            .request("releases/latest", "application/vnd.github+json", token)
            .await?;
        match response.status() {
            // GitHub also conceals private repositories from unauthorized readers with a 404.
            StatusCode::NOT_FOUND => {
                let repo = self
                    .request("", "application/vnd.github+json", token)
                    .await?;
                if repo.status() == StatusCode::OK {
                    return Ok(None);
                }
                return Err(
                    "The release repository is unavailable. Private releases require a release credential with Contents read access.",
                );
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(
                    "GitHub denied the Windows release check. Check release access or retry later.",
                );
            }
            _ => {}
        }
        let mut raw = Vec::new();
        stream_body(response, &mut raw, MAX_RELEASE).await?;
        let release: Release = serde_json::from_slice(&raw).map_err(|_| INVALID)?;
        if release.draft || release.prerelease {
            return Ok(None);
        }
        let version = release
            .tag_name
            .strip_prefix('v')
            .and_then(|v| Version::parse(v).ok())
            .ok_or(INVALID)?;
        if !version.pre.is_empty() || !version.cmp_precedence(&current.version).is_gt() {
            return Ok(None);
        }
        let dir = self
            .source
            .base
            .join(&format!("v{version}/"))
            .map_err(|_| INVALID)?;
        let (manifest_asset, manifest_hash) = release.asset(
            MANIFEST_NAME,
            &dir.join(MANIFEST_NAME).map_err(|_| INVALID)?,
            MAX_MANIFEST,
        )?;
        let selection = Selection {
            version,
            channel: "stable".into(),
            manifest_hash,
        };
        raw.clear();
        self.asset_bytes(manifest_asset, &mut raw, token).await?;
        let (manifest, url) = self.source.select(&raw, &selection, host)?;
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
            return Err("The installer asset differs from the approved Windows release manifest.");
        }
        let (dir, path, mut output) = new_download(temp)?;
        self.asset_bytes(setup, &mut output, token).await?;
        finish_download(dir, path, output, setup.size, manifest).map(Some)
    }
}

#[cfg(test)]
mod tests;
