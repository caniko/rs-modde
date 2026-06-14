use super::*;

/// A test classifier that classifies by extension.
struct TestClassifier;

impl CollisionClassifier for TestClassifier {
    fn index_archive(&self, _path: &Path) -> Result<Vec<(String, u64)>> {
        Ok(Vec::new())
    }

    fn classify_severity(&self, file_path: &str) -> CollisionSeverity {
        let ext = file_path.rsplit('.').next().unwrap_or("");
        match ext {
            "esp" | "esm" | "dll" => CollisionSeverity::Dangerous,
            "ini" | "cfg" => CollisionSeverity::Config,
            "dds" | "nif" => CollisionSeverity::Cosmetic,
            _ => CollisionSeverity::Unknown,
        }
    }

    fn archive_extensions(&self) -> &[&str] {
        &["bsa"]
    }
}

fn mod_id(s: &str) -> ModId {
    ModId::from(s)
}

#[test]
fn missing_store_dirs_summary_is_none_for_empty_input() {
    assert_eq!(summarize_missing_store_dirs(&[]), None);
}

#[test]
fn missing_store_dirs_summary_reports_all_entries_within_sample_limit() {
    let missing_mods = vec![mod_id("mod_a"), mod_id("mod_b")];

    assert_eq!(
        summarize_missing_store_dirs(&missing_mods),
        Some(MissingStoreDirsSummary {
            missing_mod_count: 2,
            missing_mod_sample: vec!["mod_a".to_string(), "mod_b".to_string()],
            omitted_missing_mod_count: 0,
        })
    );
}

#[test]
fn missing_store_dirs_summary_bounds_sample_and_counts_omitted_entries() {
    let missing_mods = (0..12)
        .map(|idx| mod_id(&format!("mod_{idx}")))
        .collect::<Vec<_>>();

    assert_eq!(
        summarize_missing_store_dirs(&missing_mods),
        Some(MissingStoreDirsSummary {
            missing_mod_count: 12,
            missing_mod_sample: (0..10).map(|idx| format!("mod_{idx}")).collect(),
            omitted_missing_mod_count: 2,
        })
    );
}

#[test]
fn build_full_conflict_map_skips_missing_store_dirs() {
    let store = tempfile::tempdir().unwrap();
    let present = store.path().join("mod_present");
    std::fs::create_dir_all(present.join("textures")).unwrap();
    std::fs::write(present.join("textures/sky.dds"), b"sky").unwrap();

    let order = vec![mod_id("mod_missing"), mod_id("mod_present")];
    let result = build_full_conflict_map(store.path(), &order, &TestClassifier).unwrap();

    assert_eq!(result.missing_mods, vec![mod_id("mod_missing")]);
    assert_eq!(result.conflict_map.files.len(), 1);
    assert!(result.conflict_map.files.contains_key("textures/sky.dds"));
    assert!(
        !result
            .conflict_map
            .files
            .values()
            .any(|mods| { mods.iter().any(|mod_id| mod_id.as_str() == "mod_missing") })
    );
    assert!(
        !result
            .origins
            .values()
            .any(|mods| { mods.keys().any(|mod_id| mod_id.as_str() == "mod_missing") })
    );
}

#[test]
fn no_collisions_produces_empty_report() {
    let mut cm = ConflictMap::default();
    cm.register("textures/sky.dds".into(), mod_id("mod_a"));
    cm.register("textures/ground.dds".into(), mod_id("mod_b"));

    let order = vec![mod_id("mod_a"), mod_id("mod_b")];
    let hidden = HashSet::new();
    let origins = HashMap::new();

    let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
    assert!(report.pairs.is_empty());
    assert_eq!(report.total_collisions, 0);
}

#[test]
fn simple_collision_detected() {
    let mut cm = ConflictMap::default();
    cm.register("textures/sky.dds".into(), mod_id("mod_a"));
    cm.register("textures/sky.dds".into(), mod_id("mod_b"));

    let order = vec![mod_id("mod_a"), mod_id("mod_b")];
    let hidden = HashSet::new();
    let origins = HashMap::new();

    let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
    assert_eq!(report.pairs.len(), 1);
    assert_eq!(report.total_collisions, 1);
    assert_eq!(report.pairs[0].winner, mod_id("mod_b"));
    assert_eq!(report.pairs[0].loser, mod_id("mod_a"));
    assert_eq!(report.pairs[0].max_severity, CollisionSeverity::Cosmetic);
}

#[test]
fn dangerous_collision_severity() {
    let mut cm = ConflictMap::default();
    cm.register("scripts/combat.esp".into(), mod_id("mod_a"));
    cm.register("scripts/combat.esp".into(), mod_id("mod_b"));

    let order = vec![mod_id("mod_a"), mod_id("mod_b")];
    let hidden = HashSet::new();
    let origins = HashMap::new();

    let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
    assert_eq!(report.pairs[0].max_severity, CollisionSeverity::Dangerous);
}

#[test]
fn shadowed_mod_detected() {
    let mut cm = ConflictMap::default();
    // mod_a provides two files, both also provided by mod_b
    cm.register("textures/sky.dds".into(), mod_id("mod_a"));
    cm.register("textures/sky.dds".into(), mod_id("mod_b"));
    cm.register("textures/ground.dds".into(), mod_id("mod_a"));
    cm.register("textures/ground.dds".into(), mod_id("mod_b"));

    let order = vec![mod_id("mod_a"), mod_id("mod_b")];
    let hidden = HashSet::new();
    let origins = HashMap::new();

    let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
    assert_eq!(report.shadowed_mods.len(), 1);
    assert_eq!(report.shadowed_mods[0].mod_id, mod_id("mod_a"));
    assert_eq!(report.shadowed_mods[0].file_count, 2);
}

#[test]
fn redundant_files_tracked() {
    let mut cm = ConflictMap::default();
    cm.register("textures/sky.dds".into(), mod_id("mod_a"));
    cm.register("textures/sky.dds".into(), mod_id("mod_b"));

    let order = vec![mod_id("mod_a"), mod_id("mod_b")];
    let hidden = HashSet::new();
    let origins = HashMap::new();

    let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
    assert_eq!(report.redundant_files.len(), 1);
    assert_eq!(report.redundant_files[0].0, mod_id("mod_a"));
}

#[test]
fn loose_vs_archive_detected() {
    let mut cm = ConflictMap::default();
    cm.register("textures/sky.dds".into(), mod_id("mod_a"));
    cm.register("textures/sky.dds".into(), mod_id("mod_b"));

    let order = vec![mod_id("mod_a"), mod_id("mod_b")];
    let hidden = HashSet::new();

    let mut origins: OriginMap = HashMap::new();
    origins
        .entry("textures/sky.dds".into())
        .or_default()
        .insert(
            mod_id("mod_a"),
            FileOrigin::Archive {
                archive_rel: "mod_a.bsa".into(),
            },
        );
    origins
        .entry("textures/sky.dds".into())
        .or_default()
        .insert(mod_id("mod_b"), FileOrigin::Loose);

    let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
    assert_eq!(report.loose_vs_archive.len(), 1);
}

#[test]
fn three_way_collision() {
    let mut cm = ConflictMap::default();
    cm.register("textures/sky.dds".into(), mod_id("mod_a"));
    cm.register("textures/sky.dds".into(), mod_id("mod_b"));
    cm.register("textures/sky.dds".into(), mod_id("mod_c"));

    let order = vec![mod_id("mod_a"), mod_id("mod_b"), mod_id("mod_c")];
    let hidden = HashSet::new();
    let origins = HashMap::new();

    let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
    // mod_c wins, mod_a and mod_b both lose → 2 collisions, 2 pairs
    assert_eq!(report.total_collisions, 2);
    assert_eq!(report.pairs.len(), 2);
}

#[test]
fn hidden_file_excluded_from_winner() {
    let mut cm = ConflictMap::default();
    cm.register("textures/sky.dds".into(), mod_id("mod_a"));
    cm.register("textures/sky.dds".into(), mod_id("mod_b"));

    let order = vec![mod_id("mod_a"), mod_id("mod_b")];
    let mut hidden = HashSet::new();
    hidden.insert(("mod_b".to_string(), "textures/sky.dds".to_string()));
    let origins = HashMap::new();

    let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
    // mod_b is hidden, so mod_a wins — still a collision pair though
    assert_eq!(report.pairs.len(), 1);
    assert_eq!(report.pairs[0].files[0].winner, mod_id("mod_a"));
}
