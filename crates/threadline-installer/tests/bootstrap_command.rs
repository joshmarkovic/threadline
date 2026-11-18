use std::{fs, process::Command};

#[test]
fn it_021_command_explains_wrong_channel_and_invalid_manifest_pin_before_accessing_files() {
    let dir = tempfile::tempdir().unwrap();
    let retained = dir.path().join("retained-profile");
    fs::write(&retained, b"intact").unwrap();
    for (version, pin, expected) in [
        ("1.2.0-rc.1", "bad", "channel"),
        ("1.2.0", "bad", "SHA-256"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_threadline-installer"))
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
