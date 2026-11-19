use super::*;
use crate::tests::{ASSET, HttpFixture, body, digest, platform};
use serde_json::{Value, json};
use std::fs;

const LATEST: &str = "/release/releases/latest";
const META: &str = "/release/releases/assets/11";
const SETUP: &str = "/release/releases/assets/12";

fn client(server: &HttpFixture) -> Github {
    let mut github = Github::new(Trust::Authenticode(
        pin(&digest(b"fixture publisher")).unwrap(),
    ))
    .unwrap();
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
async fn stages_public_stable_assets_by_api_id_and_cleans_on_drop() {
    let server = HttpFixture::new();
    let github = client(&server);
    let tmp = tempfile::tempdir().unwrap();
    publish(&server, manifest(), |_| {});
    let staged = github
        .stage(&current(), &platform(), tmp.path())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fs::read(&staged.path).unwrap(), ASSET);
    assert_eq!(*server.requests.lock().unwrap(), [LATEST, META, SETUP]);
    let headers = server.headers.lock().unwrap();
    assert!(headers.iter().all(|h| !h.contains("authorization:")));
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
                .stage(&current(), &platform(), tmp.path())
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
                .stage(&current(), &platform(), tmp.path())
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
                .stage(&current(), &platform(), tmp.path())
                .await
                .is_err()
        );
    }
    let mut m = manifest();
    m["publisher_certificate_sha256"] = json!(digest(b"different publisher"));
    publish(&server, m, |_| {});
    assert!(
        github
            .stage(&current(), &platform(), tmp.path())
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
                .stage(&current(), &platform(), tmp.path())
                .await
                .is_err()
        );
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn checks_release_visibility_when_latest_stable_release_is_missing() {
    let server = HttpFixture::new();
    let github = client(&server);
    let tmp = tempfile::tempdir().unwrap();
    assert!(
        github
            .stage(&current(), &platform(), tmp.path())
            .await
            .is_err()
    );
    server.serve("/release/releases?per_page=1", "200 OK", 2, b"[]");
    assert!(
        github
            .stage(&current(), &platform(), tmp.path())
            .await
            .unwrap()
            .is_none()
    );
    server.serve(LATEST, "403 Forbidden", 0, b"");
    assert!(
        github
            .stage(&current(), &platform(), tmp.path())
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
    let github = Github::new(Trust::Authenticode(
        pin(&digest(b"fixture publisher")).unwrap(),
    ))
    .unwrap();
    let url = header::HeaderValue::from_static(
        "https://release-assets.githubusercontent.com/download?signed=opaque",
    );
    let request = github.cdn_download(Some(&url)).unwrap().build().unwrap();
    assert!(!request.headers().contains_key(header::AUTHORIZATION));
    assert!(!request.headers().contains_key(header::ACCEPT));
    assert!(github.cdn_download(None).is_err());
}

const PRIVATE_LIST: &str = "/release/releases?per_page=20&page=1";
const SIGNATURE: &str = "/release/releases/assets/13";

fn private_manifest() -> Value {
    let mut m = manifest();
    m.as_object_mut()
        .unwrap()
        .remove("publisher_certificate_sha256");
    m["version"] = json!("0.1.0-local.22");
    m["channel"] = json!("private-preview");
    m["package_build"] = json!(22);
    m["artifact_url"] = json!(format!(
        "{DOWNLOADS}v0.1.0-local.22/Threadline-Setup-22.exe"
    ));
    m
}

fn github_signer(server: &HttpFixture) -> (Github, ring::signature::EcdsaKeyPair) {
    use ring::{
        rand::SystemRandom,
        signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair},
    };
    let rng = SystemRandom::new();
    let doc = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let pair =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, doc.as_ref(), &rng).unwrap();
    let hex = pair
        .public_key()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut github = Github::new(Trust::Private(private_key(&hex).unwrap())).unwrap();
    github.api = server.base.clone();
    (github, pair)
}

fn private_current(build: u32) -> Installed {
    Installed {
        version: Version::parse(&format!("0.1.0-local.{build}")).unwrap(),
        build,
    }
}

fn publish_private(
    server: &HttpFixture,
    build: u32,
    raw: &[u8],
    sig: &[u8],
    setup: &[u8],
) -> Value {
    let tag = format!("v0.1.0-local.{build}");
    let asset = |id: usize, name: &str, bytes: &[u8], route| {
        server.serve(route, "200 OK", bytes.len(), bytes);
        json!({"id": id, "name": name, "state": "uploaded", "size": bytes.len(),
               "digest": format!("sha256:{}", digest(bytes)), "browser_download_url":format!("{DOWNLOADS}{tag}/{name}")})
    };
    let rel = json!({"tag_name":tag,"draft":false,"prerelease":true,"assets":[
        asset(11, PRIVATE_MANIFEST, raw, META), asset(13, PRIVATE_SIGNATURE, sig, SIGNATURE),
        asset(12, &format!("Threadline-Setup-{build}.exe"), setup, SETUP)
    ]});
    let page = serde_json::to_vec(&[&rel]).unwrap();
    server.serve(PRIVATE_LIST, "200 OK", page.len(), &page);
    rel
}

#[tokio::test]
async fn stages_public_preview_anonymously_and_checks_its_signature() {
    let server = HttpFixture::new();
    let (github, pair) = github_signer(&server);
    let (_, other) = github_signer(&server);
    let tmp = tempfile::tempdir().unwrap();
    let original = serde_json::to_vec(&private_manifest()).unwrap();
    let rng = ring::rand::SystemRandom::new();
    let signature = pair.sign(&rng, &original).unwrap();
    publish_private(&server, 22, &original, signature.as_ref(), ASSET);
    let staged = github
        .stage(&private_current(21), &platform(), tmp.path())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fs::read(&staged.path).unwrap(), ASSET);
    drop(staged);
    assert_eq!(
        *server.requests.lock().unwrap(),
        [PRIVATE_LIST, META, SIGNATURE, SETUP]
    );
    assert!(
        server
            .headers
            .lock()
            .unwrap()
            .iter()
            .all(|h| !h.contains("authorization:"))
    );
    // Replacing the GitHub release and all of its SHA-256 metadata must not confer signing authority.
    let mut changed = original.clone();
    changed.push(b' ');
    let wrong_key = other.sign(&rng, &original).unwrap();
    for (raw, sig) in [
        (&changed, signature.as_ref()),
        (&original, wrong_key.as_ref()),
        (&original, &[0; 63][..]),
    ] {
        server.requests.lock().unwrap().clear();
        publish_private(&server, 22, raw, sig, ASSET);
        assert!(
            github
                .stage(&private_current(21), &platform(), tmp.path())
                .await
                .is_err()
        );
        assert!(!server.requests.lock().unwrap().iter().any(|r| r == SETUP));
    }
    publish_private(&server, 22, &original, signature.as_ref(), ASSET);
    server.serve(SETUP, "200 OK", ASSET.len(), &vec![0; ASSET.len()]);
    assert!(
        github
            .stage(&private_current(21), &platform(), tmp.path())
            .await
            .is_err()
    );
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn signed_private_metadata_must_bind_the_build_installer_schema_channel_and_target() {
    let server = HttpFixture::new();
    let (github, pair) = github_signer(&server);
    let tmp = tempfile::tempdir().unwrap();
    for (field, value) in [
        ("package_build", json!(23)),
        ("current_schema", json!(1)),
        ("target", json!("aarch64-pc-windows-msvc")),
        (
            "artifact_url",
            json!(format!("{DOWNLOADS}v0.1.0-local.22/Other.exe")),
        ),
        (
            "artifact_url",
            json!(
                "https://github.com/elsewhere/threadline/releases/download/v0.1.0-local.22/Threadline-Setup-22.exe"
            ),
        ),
        ("channel", json!("stable")),
        ("version", json!("0.1.0-local.23")),
        ("sha256", json!(digest(b"other"))),
        ("restart_protocol", json!(2)),
    ] {
        let mut m = private_manifest();
        m[field] = value;
        let raw = serde_json::to_vec(&m).unwrap();
        let sig = pair.sign(&ring::rand::SystemRandom::new(), &raw).unwrap();
        publish_private(&server, 22, &raw, sig.as_ref(), ASSET);
        assert!(
            !matches!(
                github
                    .stage(&private_current(21), &platform(), tmp.path())
                    .await,
                Ok(Some(_))
            ),
            "{field}"
        );
    }
    assert!(!server.requests.lock().unwrap().iter().any(|r| r == SETUP));
}

#[tokio::test]
async fn handles_unavailable_releases_and_rate_limits_without_authentication() {
    let server = HttpFixture::new();
    let (github, pair) = github_signer(&server);
    let tmp = tempfile::tempdir().unwrap();
    let current = private_current(21);
    let host = platform();
    let check = || github.stage(&current, &host, tmp.path());
    for status in [
        "401 Unauthorized",
        "404 Not Found",
        "403 Forbidden",
        "429 Too Many Requests",
        "500 Internal Server Error",
    ] {
        server.serve(PRIVATE_LIST, status, 0, b"");
        assert!(check().await.is_err(), "{status}");
    }
    let raw = serde_json::to_vec(&private_manifest()).unwrap();
    let sig = pair.sign(&ring::rand::SystemRandom::new(), &raw).unwrap();
    publish_private(&server, 22, &raw, sig.as_ref(), ASSET);
    server.serve(META, "404 Not Found", 0, b"");
    assert!(matches!(
        check().await,
        Err("The release asset is no longer available. Check for updates again.")
    ));
    assert!(
        server
            .headers
            .lock()
            .unwrap()
            .iter()
            .all(|h| !h.contains("authorization:"))
    );
}

#[tokio::test]
async fn preview_feed_skips_older_unsigned_prereleases() {
    let server = HttpFixture::new();
    let (github, pair) = github_signer(&server);
    let tmp = tempfile::tempdir().unwrap();
    let old = json!({"tag_name":"v0.1.0-local.20","prerelease":true,"draft":false,"assets":[]});
    let mut draft = old.clone();
    draft["tag_name"] = json!("v0.1.0-local.55");
    draft["draft"] = json!(true);
    let mut foreign = old.clone();
    foreign["tag_name"] = json!("v1.0.0-rc.999");
    let mut stable = old.clone();
    stable["tag_name"] = json!("v2.0.0");
    stable["prerelease"] = json!(false);
    let mut page = vec![old, draft, foreign, stable];
    let serve = |route, items: &[Value]| {
        let bytes = serde_json::to_vec(items).unwrap();
        server.serve(route, "200 OK", bytes.len(), &bytes);
    };
    serve(PRIVATE_LIST, &page);
    assert!(
        github
            .stage(&private_current(21), &platform(), tmp.path())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| r == PRIVATE_LIST)
    );
    let raw = serde_json::to_vec(&private_manifest()).unwrap();
    let sig = pair.sign(&ring::rand::SystemRandom::new(), &raw).unwrap();
    let valid = publish_private(&server, 22, &raw, sig.as_ref(), ASSET);
    page.resize(PAGE_SIZE, page[0].clone());
    serve(PRIVATE_LIST, &page);
    serve("/release/releases?per_page=20&page=2", &[valid]);
    assert!(
        github
            .stage(&private_current(21), &platform(), tmp.path())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
#[ignore = "requires locally downloaded release assets and their trusted public key"]
async fn verify_downloaded_private_release() {
    let dir = std::path::PathBuf::from(std::env::var_os("THREADLINE_VERIFY_RELEASE_DIR").unwrap());
    let build: u32 = std::env::var("THREADLINE_VERIFY_RELEASE_BUILD")
        .unwrap()
        .parse()
        .unwrap();
    let key = private_key(&std::env::var("THREADLINE_VERIFY_RELEASE_PUBLIC_KEY").unwrap()).unwrap();
    let setup = fs::read(dir.join(format!("Threadline-Setup-{build}.exe"))).unwrap();
    let raw = fs::read(dir.join(PRIVATE_MANIFEST)).unwrap();
    let signature = fs::read(dir.join(PRIVATE_SIGNATURE)).unwrap();
    let server = HttpFixture::new();
    publish_private(&server, build, &raw, &signature, &setup);
    let mut github = Github::new(Trust::Private(key)).unwrap();
    github.api = server.base.clone();
    let tmp = tempfile::tempdir().unwrap();
    let host = Platform {
        profile_schema: None,
        highest_build: build - 1,
        ..platform()
    };
    let staged = github
        .stage(&private_current(build - 1), &host, tmp.path())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(staged.manifest.package_build, build);
    assert_eq!(fs::read(&staged.path).unwrap(), setup);
}

#[tokio::test]
#[ignore = "downloads the public GitHub preview and checks it against the expected package"]
async fn verify_live_public_release() {
    let build: u32 = std::env::var("THREADLINE_VERIFY_RELEASE_BUILD")
        .unwrap()
        .parse()
        .unwrap();
    assert!(build > 1);
    let key = private_key(&std::env::var("THREADLINE_VERIFY_RELEASE_PUBLIC_KEY").unwrap()).unwrap();
    let expected_hash = pin(&std::env::var("THREADLINE_VERIFY_RELEASE_SHA256").unwrap()).unwrap();
    let host = Platform {
        profile_schema: None,
        highest_build: build - 1,
        ..platform()
    };
    let tmp = tempfile::tempdir().unwrap();
    let staged = Github::new(Trust::Private(key))
        .unwrap()
        .stage(&private_current(build - 1), &host, tmp.path())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(staged.manifest.package_build, build);
    assert_eq!(
        Sha256::digest(fs::read(&staged.path).unwrap()).as_slice(),
        expected_hash
    );
}
