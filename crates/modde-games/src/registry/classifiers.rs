use crate::policies::{CollisionPolicy, PolicyCollisionClassifier};

pub(super) fn bethesda_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier>
{
    Box::new(crate::bethesda::collision::BethesdaCollisionClassifier)
}

pub(super) fn cyberpunk_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier>
{
    Box::new(crate::cyberpunk::collision::CyberpunkCollisionClassifier)
}

pub(super) fn ue4_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier> {
    Box::new(crate::policies::PolicyCollisionClassifier {
        policy: crate::ue4::UE4_COLLISION_POLICY,
    })
}

fn policy_collision_classifier(
    archive_extensions: &'static [&'static str],
) -> Box<dyn modde_core::collision::CollisionClassifier> {
    Box::new(PolicyCollisionClassifier {
        policy: CollisionPolicy {
            archive_extensions,
            severities: DEFAULT_SEVERITIES,
        },
    })
}

const DEFAULT_ARCHIVE_EXTENSIONS: &[&str] = &[];
const BSA_ARCHIVE_EXTENSIONS: &[&str] = &["bsa"];
const PAK_ARCHIVE_EXTENSIONS: &[&str] = &["pak", "ucas", "utoc"];
const WITCHER_ARCHIVE_EXTENSIONS: &[&str] = &["bundle", "cache"];

const DEFAULT_SEVERITIES: &[(&str, modde_core::collision::CollisionSeverity)] = &[
    ("dds", modde_core::collision::CollisionSeverity::Cosmetic),
    ("png", modde_core::collision::CollisionSeverity::Cosmetic),
    ("jpg", modde_core::collision::CollisionSeverity::Cosmetic),
    ("tga", modde_core::collision::CollisionSeverity::Cosmetic),
    ("nif", modde_core::collision::CollisionSeverity::Cosmetic),
    ("ini", modde_core::collision::CollisionSeverity::Config),
    ("json", modde_core::collision::CollisionSeverity::Config),
    ("xml", modde_core::collision::CollisionSeverity::Config),
    ("esp", modde_core::collision::CollisionSeverity::Dangerous),
    ("esm", modde_core::collision::CollisionSeverity::Dangerous),
    ("dll", modde_core::collision::CollisionSeverity::Dangerous),
    ("lua", modde_core::collision::CollisionSeverity::Dangerous),
    ("ws", modde_core::collision::CollisionSeverity::Dangerous),
];

pub(crate) fn generic_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier>
{
    policy_collision_classifier(DEFAULT_ARCHIVE_EXTENSIONS)
}

pub(super) fn gamebryo_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier>
{
    policy_collision_classifier(BSA_ARCHIVE_EXTENSIONS)
}

pub(super) fn pak_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier> {
    policy_collision_classifier(PAK_ARCHIVE_EXTENSIONS)
}

pub(super) fn witcher_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier>
{
    policy_collision_classifier(WITCHER_ARCHIVE_EXTENSIONS)
}
