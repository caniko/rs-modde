mod common;

use std::io::Write;

use common::Fixture;
use serde_json::Value;
use xxhash_rust::xxh64::xxh64;

fn write_wabbajack(path: &std::path::Path, manifest: Value) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("modlist", options).unwrap();
    writer
        .write_all(serde_json::to_string(&manifest).unwrap().as_bytes())
        .unwrap();
    writer.finish().unwrap();
}

fn manifest_with_archive(hash: u64, archive: Value) -> Value {
    serde_json::json!({
        "Name": "AcquireTest",
        "Author": "tester",
        "Description": "desc",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [
            {
                "Hash": hash,
                "Name": archive["name"],
                "Size": archive["size"],
                "State": archive["state"],
            }
        ],
        "Directives": [],
    })
}

#[test]
fn acquire_missing_json_returns_empty_when_store_has_archive() {
    let fx = Fixture::new();
    let bytes = b"already present archive";
    let hash = xxh64(bytes, 0);
    let manifest = manifest_with_archive(
        hash,
        serde_json::json!({
            "name": "manual.7z",
            "size": bytes.len(),
            "state": {
                "$type": "ManualDownloader, Wabbajack.Lib",
                "Url": "https://example.test/manual.7z",
                "Prompt": "",
            },
        }),
    );
    let manifest_path = fx.root().join("list.wabbajack");
    write_wabbajack(&manifest_path, manifest);
    let store = fx.data_dir().join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join(format!("{hash:016x}.archive")), bytes).unwrap();

    let output = fx
        .cmd()
        .args([
            "wabbajack",
            "acquire-missing",
            manifest_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[test]
fn acquire_missing_json_reports_nexus_credentials_missing() {
    let fx = Fixture::new();
    let bytes = b"nexus archive";
    let hash = xxh64(bytes, 0);
    let manifest = manifest_with_archive(
        hash,
        serde_json::json!({
            "name": "nexus.7z",
            "size": bytes.len(),
            "state": {
                "$type": "NexusDownloader, Wabbajack.Lib",
                "GameName": "ModdingTools",
                "ModID": 631,
                "FileID": 5118,
            },
        }),
    );
    let manifest_path = fx.root().join("list.wabbajack");
    write_wabbajack(&manifest_path, manifest);

    let output = fx
        .cmd()
        .args([
            "wabbajack",
            "acquire-missing",
            manifest_path.to_str().unwrap(),
            "--include-nexus",
            "--json",
        ])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let entry = &json.as_array().unwrap()[0];
    assert_eq!(entry["status"], "nexus-credentials-missing");
    assert_eq!(
        entry["archive"]["source_hint"],
        "Nexus ModdingTools mod_id=631 file_id=5118"
    );
}

#[test]
fn missing_impact_json_reports_manual_archive_metrics() {
    let fx = Fixture::new();
    let hash = xxh64(b"manual archive", 0);
    let manifest = serde_json::json!({
        "Name": "ImpactTest",
        "Author": "tester",
        "Description": "desc",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [
            {
                "Hash": hash,
                "Name": "manual.7z",
                "Size": 14,
                "State": {
                    "$type": "ManualDownloader, Wabbajack.Lib",
                    "Url": "https://example.test/manual.7z",
                    "Prompt": "",
                },
            }
        ],
        "Directives": [
            {
                "$type": "FromArchive",
                "ArchiveHashPath": [hash, "a.txt"],
                "To": "mods\\\\Missing Mod\\\\a.txt",
                "Size": 5,
            }
        ],
    });
    let manifest_path = fx.root().join("impact.wabbajack");
    write_wabbajack(&manifest_path, manifest);

    let output = fx
        .cmd()
        .args([
            "wabbajack",
            "missing-impact",
            manifest_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["missing_archives"].as_array().unwrap().len(), 1);
    assert_eq!(json["blocked_archive_directives"], 1);
    assert_eq!(json["blocked_output_bytes"], 5);
    assert_eq!(json["affected_mod_roots"][0]["name"], "Missing Mod");
}

#[test]
fn missing_impact_nix_snippet_uses_readable_archive_keys() {
    let fx = Fixture::new();
    let hash = xxh64(b"manual archive", 0);
    let manifest = serde_json::json!({
        "Name": "ImpactTest",
        "Author": "tester",
        "Description": "desc",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [
            {
                "Hash": hash,
                "Name": "[COCO] Scarlet Rose [SE] - CBBE.7z",
                "Size": 14,
                "State": {
                    "$type": "ManualDownloader, Wabbajack.Lib",
                    "Url": "https://example.test/manual.7z",
                    "Prompt": "",
                },
            }
        ],
        "Directives": [],
    });
    let manifest_path = fx.root().join("impact.wabbajack");
    write_wabbajack(&manifest_path, manifest);

    let output = fx
        .cmd()
        .args([
            "wabbajack",
            "missing-impact",
            manifest_path.to_str().unwrap(),
            "--nix-snippet",
        ])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("manualArchives = {"));
    assert!(stdout.contains("\"[COCO] Scarlet Rose [SE] - CBBE.7z\" = {"));
    assert!(stdout.contains(&format!("hash = \"{hash:016x}\";")));
    assert!(stdout.contains("# source: https://example.test/manual.7z"));
    assert!(stdout.contains("# expected size: 14 bytes"));
    assert!(stdout.contains("optional = true;"));
}

#[test]
fn manual_links_json_reports_known_missing_manual_hosts_only() {
    let fx = Fixture::new();
    let present_hash = xxh64(b"present", 0);
    let workupload_hash = xxh64(b"workupload", 0);
    let sharemods_hash = xxh64(b"sharemods", 0);
    let loverslab_hash = xxh64(b"loverslab", 0);
    let direct_hash = xxh64(b"direct", 0);
    let invalid_hash = xxh64(b"invalid", 0);
    let manifest = serde_json::json!({
        "Name": "ManualLinksTest",
        "Author": "tester",
        "Description": "desc",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [
            manual_archive(workupload_hash, "workupload.7z", "https://workupload.com/file/abc"),
            manual_archive(sharemods_hash, "sharemods.7z", "https://www.sharemods.com/x/file.7z.html"),
            manual_archive(loverslab_hash, "loverslab.7z", "https://www.loverslab.com/files/file/5051-halo-poser-se/"),
            manual_archive(direct_hash, "direct.7z", "https://example.test/direct.7z"),
            manual_archive(invalid_hash, "invalid.7z", "not a url"),
            manual_archive(present_hash, "present.7z", "https://workupload.com/file/present"),
        ],
        "Directives": [],
    });
    let manifest_path = fx.root().join("manual-links.wabbajack");
    write_wabbajack(&manifest_path, manifest);
    let store = fx.data_dir().join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(
        store.join(format!("{present_hash:016x}.archive")),
        b"present",
    )
    .unwrap();

    let output = fx
        .cmd()
        .args([
            "wabbajack",
            "manual-links",
            manifest_path.to_str().unwrap(),
            "--data-dir",
            fx.data_dir().to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let entries = json.as_array().unwrap();
    assert_eq!(entries.len(), 3);
    let domains = entries
        .iter()
        .map(|entry| entry["domain"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        domains,
        ["workupload.com", "sharemods.com", "loverslab.com"]
    );
    assert_eq!(entries[0]["hash_hex"], format!("{workupload_hash:016x}"));
    assert_eq!(entries[0]["url"], "https://workupload.com/file/abc");
    assert_eq!(
        entries[0]["store_path"],
        store
            .join(format!("{workupload_hash:016x}.archive"))
            .display()
            .to_string()
    );
}

#[test]
fn manual_links_text_prints_operator_urls() {
    let fx = Fixture::new();
    let hash = xxh64(b"workupload", 0);
    let manifest = serde_json::json!({
        "Name": "ManualLinksTextTest",
        "Author": "tester",
        "Description": "desc",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [
            manual_archive(hash, "workupload.7z", "https://workupload.com/file/abc"),
        ],
        "Directives": [],
    });
    let manifest_path = fx.root().join("manual-links-text.wabbajack");
    write_wabbajack(&manifest_path, manifest);

    let output = fx
        .cmd()
        .args(["wabbajack", "manual-links", manifest_path.to_str().unwrap()])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("workupload.7z"));
    assert!(stdout.contains(&format!("{hash:016x}")));
    assert!(stdout.contains("workupload.com"));
    assert!(stdout.contains("https://workupload.com/file/abc"));
}

fn manual_archive(hash: u64, name: &str, url: &str) -> Value {
    serde_json::json!({
        "Hash": hash,
        "Name": name,
        "Size": 14,
        "State": {
            "$type": "ManualDownloader, Wabbajack.Lib",
            "Url": url,
            "Prompt": "",
        },
    })
}
