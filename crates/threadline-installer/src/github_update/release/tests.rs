use super::*;
use crate::tests::{ASSET, HttpFixture, body, digest, platform};
use serde_json::{Value, json};
use std::fs;

const LATEST: &str = "/release/releases/latest";
const META: &str = "/release/releases/assets/11";
const SETUP: &str = "/release/releases/assets/12";

fn client(server: &HttpFixture) -> Github {
    let mut github = Github::new(&digest(b"fixture publisher")).unwrap();
    github.api = server.base.clone();
    github
}

fn manifest() -> Value {
    let mut manifest = body(&Url::parse(DOWNLOADS).unwrap());
    manifest["restart_protocol"] = json!(1);
    manifest
}

fn publish(server: &HttpFixture, manifest: Value, change: impl FnOnce(&mut Value)) {
    let raw = serde_json::to_vec(&manifest).unwrap();
    let asset = |id: u32, name: &str, bytes: &[u8]| {
        json!({"id":id,"name":name,"state":"uploaded","size":bytes.len(),
           "digest":format!("sha256:{}",digest(bytes)), "browser_download_url":format!("{DOWNLOADS}v1.2.3/{name}")})
    };
    let mut release = json!({"tag_name":"v1.2.3","draft":false,"prerelease":false,
        "assets":[asset(11, MANIFEST_NAME, &raw), asset(12, "ThreadlineSetup.exe", ASSET)]});
    change(&mut release);
    let release = serde_json::to_vec(&release).unwrap();
    server.serve(LATEST, "200 OK", release.len(), &release);
    server.serve(META, "200 OK", raw.len(), &raw);
    server.serve(SETUP, "200 OK", ASSET.len(), ASSET);
}

fn current() -> Installed {
    Installed {
        version: Version::parse("1.1.9").unwrap(),
        build: 2,
    }
}

#[tokio::test]
async fn stages_private_assets_by_api_id_and_cleans_on_drop() {
    let server = HttpFixture::new();
    let github = client(&server);
    let tmp = tempfile::tempdir().unwrap();
    publish(&server, manifest(), |_| {});
    let staged = github
        .stage(
            &current(),
            &platform(),
            tmp.path(),
            Some("fixture-release-only"),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fs::read(&staged.path).unwrap(), ASSET);
    assert_eq!(*server.requests.lock().unwrap(), [LATEST, META, SETUP]);
    let headers = server.headers.lock().unwrap();
    assert!(
        headers
            .iter()
            .all(|h| h.contains("authorization: bearer fixture-release-only\r\n"))
    );
    assert!(headers[0].contains("accept: application/vnd.github+json"));
    assert!(
        headers[1..]
            .iter()
            .all(|h| h.contains("accept: application/octet-stream"))
    );
    drop(headers);
    #[cfg(windows)]
    assert!(
        staged.launch_for_restart().is_err(),
        "digest alone cannot authorize restart"
    );
    let path = staged.path.clone();
    drop(staged);
    assert!(!path.exists());
}

#[tokio::test]
async fn ignores_non_stable_older_and_protocol_incompatible_releases() {
    let server = HttpFixture::new();
    let github = client(&server);
    let tmp = tempfile::tempdir().unwrap();
    for (field, value) in [
        ("draft", json!(true)),
        ("prerelease", json!(true)),
        ("tag_name", json!("v1.3.0-rc.1")),
        ("tag_name", json!("v1.1.9+metadata")),
        ("tag_name", json!("v1.1.8")),
    ] {
        publish(&server, manifest(), |r| r[field] = value);
        assert!(
            github
                .stage(&current(), &platform(), tmp.path(), None)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|route| route == LATEST)
    );
    for (field, value) in [
        ("restart_protocol", Value::Null),
        ("restart_protocol", json!(2)),
        ("package_build", json!(platform().highest_build)),
    ] {
        let mut m = manifest();
        m[field] = value;
        publish(&server, m, |_| {});
        assert!(
            github
                .stage(&current(), &platform(), tmp.path(), None)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert!(
        !server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|route| route == SETUP)
    );
}

#[tokio::test]
async fn rejects_ambiguous_unsigned_or_corrupt_assets_before_handoff() {
    let server = HttpFixture::new();
    let github = client(&server);
    let tmp = tempfile::tempdir().unwrap();
    for (index, field, value) in [
        (1, "id", json!(11)),
        (1, "digest", Value::Null),
        (1, "digest", json!(format!("sha256:{}", digest(b"changed")))),
        (1, "size", json!(ASSET.len() - 1)),
        (0, "state", json!("new")),
        (
            0,
            "browser_download_url",
            json!("https://example.invalid/private"),
        ),
        (
            0,
            "digest",
            json!(format!("sha256:{}", digest(b"corrupt manifest"))),
        ),
        (0, "name", json!("ThreadlineSetup.exe")),
    ] {
        publish(&server, manifest(), |r| r["assets"][index][field] = value);
        assert!(
            github
                .stage(&current(), &platform(), tmp.path(), None)
                .await
                .is_err()
        );
    }
    let mut m = manifest();
    m["publisher_certificate_sha256"] = json!(digest(b"different publisher"));
    publish(&server, m, |_| {});
    assert!(
        github
            .stage(&current(), &platform(), tmp.path(), None)
            .await
            .is_err()
    );
    assert!(
        !server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|route| route == SETUP)
    );
    publish(&server, manifest(), |_| {});
    for bytes in [ASSET[1..].to_vec(), vec![0; ASSET.len()]] {
        server.serve(SETUP, "200 OK", ASSET.len(), &bytes);
        assert!(
            github
                .stage(&current(), &platform(), tmp.path(), None)
                .await
                .is_err()
        );
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn checks_repo_visibility_when_latest_release_is_missing() {
    let server = HttpFixture::new();
    let github = client(&server);
    let tmp = tempfile::tempdir().unwrap();
    assert!(
        github
            .stage(&current(), &platform(), tmp.path(), None)
            .await
            .is_err()
    );
    server.serve("/release", "200 OK", 2, b"{}");
    assert!(
        github
            .stage(&current(), &platform(), tmp.path(), Some("fixture"))
            .await
            .unwrap()
            .is_none()
    );
    server.serve(LATEST, "403 Forbidden", 0, b"");
    assert!(
        github
            .stage(&current(), &platform(), tmp.path(), Some("fixture"))
            .await
            .is_err()
    );
}

#[test]
fn rejects_untrusted_redirect_targets() {
    for target in [
        "http://release-assets.githubusercontent.com/download",
        "https://release-assets.githubusercontent.com.example.invalid/download",
        "https://release-assets.githubusercontent.com:8443/download",
        "https://api.github.com/download",
        "https://user@release-assets.githubusercontent.com/download",
        "https://github.com/download",
    ] {
        assert!(!allowed_cdn(&Url::parse(target).unwrap()));
    }
    assert!(allowed_cdn(
        &Url::parse("https://release-assets.githubusercontent.com/download?signed=opaque").unwrap()
    ));
}

#[test]
fn asset_redirects_discard_api_authorization_and_accept_headers() {
    let github = Github::new(&digest(b"fixture publisher")).unwrap();
    let url = header::HeaderValue::from_static(
        "https://release-assets.githubusercontent.com/download?signed=opaque",
    );
    let request = github.cdn_download(Some(&url)).unwrap().build().unwrap();
    assert!(!request.headers().contains_key(header::AUTHORIZATION));
    assert!(!request.headers().contains_key(header::ACCEPT));
    assert!(github.cdn_download(None).is_err());
}
