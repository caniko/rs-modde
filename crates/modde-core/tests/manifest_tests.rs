use modde_core::manifest::collection::CollectionManifest;
use modde_core::manifest::wabbajack::{DownloadDirective, WabbajackManifest};

#[test]
fn parse_wabbajack_manifest() {
    let json = include_str!("fixtures/wabbajack_manifest.json");
    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();

    assert_eq!(manifest.name, "Test Modlist");
    assert_eq!(manifest.author, "TestAuthor");
    assert_eq!(manifest.game, "SkyrimSpecialEdition");
    assert_eq!(manifest.archives.len(), 3);
    assert_eq!(manifest.directives.len(), 3);
}

#[test]
fn wabbajack_download_directives() {
    let json = include_str!("fixtures/wabbajack_manifest.json");
    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();

    let downloads = manifest.download_directives();
    assert_eq!(downloads.len(), 3);

    assert!(matches!(&downloads[0], DownloadDirective::Nexus { mod_id: 42, .. }));
    assert!(matches!(&downloads[1], DownloadDirective::GitHub { repo, .. } if repo == "test-repo"));
    assert!(matches!(&downloads[2], DownloadDirective::DirectURL { .. }));
}

#[test]
fn wabbajack_install_directives() {
    let json = include_str!("fixtures/wabbajack_manifest.json");
    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();

    let installs = manifest.install_directives();
    assert_eq!(installs.len(), 3);
}

#[test]
fn parse_collection_manifest() {
    let json = include_str!("fixtures/collection_manifest.json");
    let collection: CollectionManifest = serde_json::from_str(json).unwrap();

    assert_eq!(collection.slug, "test-collection");
    assert_eq!(collection.name, "Test Collection");
    assert_eq!(collection.author.name, "TestAuthor");
    assert_eq!(collection.game.domain_name, "skyrimspecialedition");
    assert_eq!(collection.mods.len(), 2);
    assert_eq!(collection.endorsements, 42);

    assert!(!collection.mods[0].optional);
    assert!(collection.mods[1].optional);
    assert!(collection.mods[1].patch.is_some());
}
