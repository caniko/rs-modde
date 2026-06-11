mod common;

use common::Fixture;

#[test]
fn lock_export_sign_verify_round_trips_empty_profile() {
    let fx = Fixture::new();
    let lock = fx.root().join("modde.lock");
    let public_key = fx.root().join("modde-lock.pub.json");
    let secret_key = fx.root().join("modde-lock.secret.json");

    let create = fx
        .cmd()
        .args(["profile", "create", "portable", "--game", "skyrim-se"])
        .output()
        .expect("profile create");
    assert!(
        create.status.success(),
        "profile create failed: {}",
        String::from_utf8_lossy(&create.stderr)
    );

    let export = fx
        .cmd()
        .args([
            "lock",
            "export",
            "--profile",
            "portable",
            "--game",
            "skyrim-se",
            "--output",
            lock.to_str().unwrap(),
        ])
        .output()
        .expect("lock export");
    assert!(
        export.status.success(),
        "lock export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    let exported = std::fs::read_to_string(&lock).unwrap();
    assert!(exported.contains("\"kind\": \"modde.lock\""));
    assert!(exported.contains("\"format_version\": 1"));

    let keygen = fx
        .cmd()
        .args([
            "lock",
            "keygen",
            "--public",
            public_key.to_str().unwrap(),
            "--secret",
            secret_key.to_str().unwrap(),
        ])
        .output()
        .expect("lock keygen");
    assert!(
        keygen.status.success(),
        "lock keygen failed: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );

    let sign = fx
        .cmd()
        .args([
            "lock",
            "sign",
            lock.to_str().unwrap(),
            "--secret-key",
            secret_key.to_str().unwrap(),
        ])
        .output()
        .expect("lock sign");
    assert!(
        sign.status.success(),
        "lock sign failed: {}",
        String::from_utf8_lossy(&sign.stderr)
    );

    let verify = fx
        .cmd()
        .args([
            "lock",
            "verify",
            lock.to_str().unwrap(),
            "--profile",
            "portable",
            "--game",
            "skyrim-se",
        ])
        .output()
        .expect("lock verify");
    assert!(
        verify.status.success(),
        "lock verify failed: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let stdout = String::from_utf8_lossy(&verify.stdout);
    assert!(
        stdout.contains("Verified 1 signature"),
        "verify output missing signature count:\n{stdout}"
    );
}
