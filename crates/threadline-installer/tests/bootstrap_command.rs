use std::{env, fs, path::PathBuf, process::Command};

fn installer() -> PathBuf {
    env::var_os("THREADLINE_TEST_INSTALLER_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_threadline-installer")))
        .canonicalize()
        .expect("locate the Cargo installer or the explicitly staged test executable")
}

#[test]
fn it_021_command_explains_wrong_channel_and_invalid_manifest_pin_before_accessing_files() {
    let installer = installer();
    let dir = tempfile::tempdir().unwrap();
    let retained = dir.path().join("retained-profile");
    fs::write(&retained, b"intact").unwrap();
    for (version, pin, expected) in [
        ("1.2.0-rc.1", "bad", "channel"),
        ("1.2.0", "bad", "SHA-256"),
    ] {
        let output = Command::new(&installer)
            .current_dir(dir.path())
            .args([
                "--version",
                version,
                "--channel",
                "stable",
                "--manifest-sha256",
                pin,
                "--offline-dir",
                ".",
                "--verify-only",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr).unwrap().contains(expected));
        assert_eq!(fs::read(&retained).unwrap(), b"intact");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn it_019_removal_commands_reject_arbitrary_paths_before_accessing_a_profile() {
    let marker = tempfile::NamedTempFile::new().unwrap();
    for option in [
        "--preview-local-removal",
        "--remove-local-data-and-credentials",
    ] {
        let output = Command::new(installer())
            .arg(option)
            .arg(marker.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("no additional arguments or paths")
        );
        assert!(marker.path().exists());
    }
}
