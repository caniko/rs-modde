mod common;

mod cli_merge {
    use modde_core::collision::FileOrigin;
    use modde_core::db::ModdeDb;
    use modde_core::merge::{
        BaseSource, MergeKind, MergeParticipant, MergeSession, MergeStatus,
        merge_group_for_rel_path,
    };
    use modde_core::profile::{EnabledMod, Profile, ProfileSource};
    use modde_core::resolver::GameId;

    use crate::common::Fixture;

    fn profile() -> Profile {
        Profile {
            id: None,
            name: "test".to_string(),
            game_id: GameId::from("skyrim-se"),
            source: ProfileSource::Manual,
            mods: vec![
                EnabledMod {
                    mod_id: "low".to_string(),
                    enabled: true,
                    ..Default::default()
                },
                EnabledMod {
                    mod_id: "high".to_string(),
                    enabled: true,
                    ..Default::default()
                },
            ],
            overrides: "/tmp/overrides".into(),
            load_order_rules: Default::default(),
            load_order_lock: None,
        }
    }

    fn session(rel_path: &str) -> MergeSession {
        session_with_syntax(rel_path, "txt")
    }

    fn session_with_syntax(rel_path: &str, syntax: &str) -> MergeSession {
        MergeSession {
            merge_group: merge_group_for_rel_path(rel_path),
            rel_path: rel_path.to_string(),
            participants: vec![
                MergeParticipant {
                    mod_id: "low".into(),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
                MergeParticipant {
                    mod_id: "high".into(),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
            ],
            base: BaseSource::Missing,
            kind: MergeKind::Text {
                syntax: syntax.to_string(),
            },
            status: MergeStatus::Pending,
            result_path: None,
            merged_with: None,
            resolved_at: None,
        }
    }

    fn seed(fixture: &Fixture) -> (i64, MergeSession) {
        let db = ModdeDb::open_at(&fixture.data_dir().join("modde.db")).unwrap();
        let profile_id = db.create_profile(&profile()).unwrap();
        let session = session("config/test.txt");
        db.upsert_merge_session(profile_id, &session).unwrap();
        (profile_id, session)
    }

    fn seed_with_session(fixture: &Fixture, session: &MergeSession) -> i64 {
        let db = ModdeDb::open_at(&fixture.data_dir().join("modde.db")).unwrap();
        let profile_id = db.create_profile(&profile()).unwrap();
        db.upsert_merge_session(profile_id, session).unwrap();
        profile_id
    }

    #[test]
    fn drivers_lists_four_rows() {
        let fixture = Fixture::new();
        let output = fixture
            .cmd()
            .args(["merge", "drivers"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let stdout = String::from_utf8(output).unwrap();

        assert_eq!(stdout.lines().count(), 4);
        assert!(stdout.contains("vscode\tVS Code\t"));
        assert!(stdout.contains("meld\tMeld\t"));
        assert!(stdout.contains("kdiff3\tKDiff3\t"));
        assert!(stdout.contains("inline\tInline manual\t"));
    }

    #[test]
    fn list_prints_seeded_session() {
        let fixture = Fixture::new();
        let (_profile_id, session) = seed(&fixture);
        let output = fixture
            .cmd()
            .args(["merge", "list", "--profile", "test", "--game", "skyrim-se"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let stdout = String::from_utf8(output).unwrap();

        assert!(stdout.contains(&session.merge_group));
        assert!(stdout.contains("pending"));
        assert!(stdout.contains("config/test.txt"));
        assert!(stdout.contains("2 mods"));
    }

    #[test]
    fn open_unknown_group_fails() {
        let fixture = Fixture::new();
        seed(&fixture);
        let output = fixture
            .cmd()
            .args([
                "merge",
                "open",
                "unknown",
                "--profile",
                "test",
                "--game",
                "skyrim-se",
            ])
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();
        let stderr = String::from_utf8(output).unwrap();

        assert!(stderr.contains("no such merge session: unknown"));
    }

    #[test]
    fn accept_winner_writes_current_winner_result() {
        let fixture = Fixture::new();
        let (profile_id, session) = seed(&fixture);
        std::fs::create_dir_all(fixture.data_dir().join("store/low/config")).unwrap();
        std::fs::create_dir_all(fixture.data_dir().join("store/high/config")).unwrap();
        std::fs::write(
            fixture.data_dir().join("store/low/config/test.txt"),
            "low\n",
        )
        .unwrap();
        std::fs::write(
            fixture.data_dir().join("store/high/config/test.txt"),
            "high\n",
        )
        .unwrap();

        fixture
            .cmd()
            .args([
                "merge",
                "accept-winner",
                &session.merge_group,
                "--profile",
                "test",
                "--game",
                "skyrim-se",
            ])
            .assert()
            .success();

        let db = ModdeDb::open_at(&fixture.data_dir().join("modde.db")).unwrap();
        let loaded = db
            .get_merge_session(profile_id, &session.merge_group)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.status, MergeStatus::Pending);
        let result_path = loaded.result_path.unwrap();
        assert_eq!(std::fs::read_to_string(result_path).unwrap(), "high\n");
    }

    #[test]
    fn cli_merge_validate_accepts_valid_xml_result() {
        let fixture = Fixture::new();
        let session = session_with_syntax("config/test.xml", "xml");
        seed_with_session(&fixture, &session);
        let result = fixture
            .data_dir()
            .join("merge-sessions")
            .join(&session.merge_group)
            .join("result.txt");
        std::fs::create_dir_all(result.parent().unwrap()).unwrap();
        std::fs::write(&result, "<root />\n").unwrap();

        let output = fixture
            .cmd()
            .args([
                "merge",
                "validate",
                &session.merge_group,
                "--profile",
                "test",
                "--game",
                "skyrim-se",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let stdout = String::from_utf8(output).unwrap();

        assert_eq!(stdout, "OK\n");
    }

    #[test]
    fn cli_merge_validate_rejects_invalid_xml_result() {
        let fixture = Fixture::new();
        let session = session_with_syntax("config/test.xml", "xml");
        seed_with_session(&fixture, &session);
        let result = fixture
            .data_dir()
            .join("merge-sessions")
            .join(&session.merge_group)
            .join("result.txt");
        std::fs::create_dir_all(result.parent().unwrap()).unwrap();
        std::fs::write(&result, "<root>\n").unwrap();

        let output = fixture
            .cmd()
            .args([
                "merge",
                "validate",
                &session.merge_group,
                "--profile",
                "test",
                "--game",
                "skyrim-se",
            ])
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();
        let stderr = String::from_utf8(output).unwrap();

        assert!(stderr.contains("XML parse error"));
    }

    #[test]
    fn merge_witcher3_set_vanilla_rejects_nonexistent_dir() {
        let fixture = Fixture::new();
        let missing = fixture.root().join("missing-vanilla");
        let output = fixture
            .cmd()
            .args(["merge", "witcher3", "set-vanilla"])
            .arg(&missing)
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();
        let stderr = String::from_utf8(output).unwrap();

        assert!(stderr.contains("vanilla cache dir does not exist"));
    }

    #[test]
    fn merge_witcher3_set_vanilla_persists_for_show_vanilla() {
        let fixture = Fixture::new();
        let cache = fixture.root().join("vanilla-cache");
        let canary = cache.join("content/scripts/game/r4Player.ws");
        std::fs::create_dir_all(canary.parent().unwrap()).unwrap();
        std::fs::write(&canary, "// vanilla").unwrap();
        let cache = std::fs::canonicalize(cache).unwrap();

        let output = fixture
            .cmd()
            .args(["merge", "witcher3", "set-vanilla"])
            .arg(&cache)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let stdout = String::from_utf8(output).unwrap();
        assert_eq!(stdout, format!("{}\n", cache.display()));

        let output = fixture
            .cmd()
            .args(["merge", "witcher3", "show-vanilla"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let stdout = String::from_utf8(output).unwrap();
        assert_eq!(stdout, format!("{}\n", cache.display()));

        let db = ModdeDb::open_at(&fixture.data_dir().join("modde.db")).unwrap();
        assert_eq!(db.get_vanilla_dir("witcher3").unwrap(), Some(cache));
    }
}
