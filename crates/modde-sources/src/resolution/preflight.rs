use super::*;

pub fn preflight_collection_transaction(
    manifest: &CollectionManifest,
) -> Result<TransactionPlan, TransactionError> {
    let provider = ModdeDependencyProvider::new();
    let mut requirements = Vec::new();

    let mut mods = manifest.mods.clone();
    mods.sort_by_key(|m| m.install_order);

    for (rank, collection_mod) in mods.into_iter().enumerate() {
        let package = TransactionPackage::NexusMod {
            game_domain: manifest.game.domain_name.clone(),
            mod_id: collection_mod.mod_id,
        };
        let version = TransactionVersion::NexusFile {
            file_id: collection_mod.file_id,
        };
        let package_key = ModdeDependencyProvider::package_key(&package);
        let artifact = TransactionArtifact {
            package,
            version: version.clone(),
            display_name: collection_mod.name,
            display_version: Some(collection_mod.version),
            enabled_by_default: !collection_mod.optional,
            provenance: TransactionProvenance::NexusCollection {
                slug: manifest.slug.clone(),
                version: manifest.version.version.clone(),
            },
        };
        provider.add_artifact(artifact, rank, Vec::new());
        if !collection_mod.optional {
            requirements.push(provider.exact_requirement(&package_key, version));
        }
    }

    let mut plan = solve_transaction(provider, requirements)?;
    // Collection optional artifacts are soft roots. Include them when their package
    // is not already fixed by a hard requirement; otherwise drop them instead of
    // failing the transaction.
    let selected_packages = plan
        .artifacts
        .iter()
        .map(|artifact| ModdeDependencyProvider::package_key(&artifact.package))
        .collect::<std::collections::HashSet<_>>();
    let mut additions = manifest
        .mods
        .iter()
        .filter(|collection_mod| collection_mod.optional)
        .filter_map(|collection_mod| {
            let package = TransactionPackage::NexusMod {
                game_domain: manifest.game.domain_name.clone(),
                mod_id: collection_mod.mod_id,
            };
            let package_key = ModdeDependencyProvider::package_key(&package);
            (!selected_packages.contains(&package_key)).then(|| {
                (
                    collection_mod.install_order,
                    TransactionArtifact {
                        package,
                        version: TransactionVersion::NexusFile {
                            file_id: collection_mod.file_id,
                        },
                        display_name: collection_mod.name.clone(),
                        display_version: Some(collection_mod.version.clone()),
                        enabled_by_default: false,
                        provenance: TransactionProvenance::NexusCollection {
                            slug: manifest.slug.clone(),
                            version: manifest.version.version.clone(),
                        },
                    },
                )
            })
        })
        .collect::<Vec<_>>();
    additions.sort_by_key(|(rank, _)| *rank);
    plan.artifacts
        .extend(additions.into_iter().map(|(_, artifact)| artifact));
    plan.artifacts.sort_by_key(|artifact| {
        manifest
            .mods
            .iter()
            .find(|m| {
                matches!(
                    &artifact.package,
                    TransactionPackage::NexusMod { mod_id, .. } if *mod_id == m.mod_id
                ) && matches!(
                    artifact.version,
                    TransactionVersion::NexusFile { file_id } if file_id == m.file_id
                )
            })
            .map_or(i32::MAX, |m| m.install_order)
    });
    Ok(plan)
}

/// Resolve one exact Nexus mod file while fetching file candidates on demand.
pub async fn preflight_lazy_nexus_file_transaction(
    api: NexusApi,
    game_domain: &str,
    mod_id: NexusModId,
    file_id: NexusFileId,
) -> Result<TransactionPlan, TransactionError> {
    let provider = ModdeDependencyProvider::with_nexus_api(api);
    let package = TransactionPackage::NexusMod {
        game_domain: game_domain.to_string(),
        mod_id,
    };
    let package_key = ModdeDependencyProvider::package_key(&package);
    provider.add_lazy_nexus_mod(game_domain.to_string(), mod_id);
    let requirements =
        vec![provider.exact_requirement(&package_key, TransactionVersion::NexusFile { file_id })];
    tokio::task::block_in_place(|| solve_transaction_with_current_runtime(provider, requirements))
}

/// Resolve a Wabbajack manifest as an exact artifact transaction.
pub fn preflight_wabbajack_transaction(
    manifest: &WabbajackManifest,
    manifest_hash: &str,
) -> Result<TransactionPlan, TransactionError> {
    let provider = ModdeDependencyProvider::new();
    let mut requirements = Vec::new();
    let provenance = TransactionProvenance::WabbajackManifest {
        manifest_hash: manifest_hash.to_string(),
    };

    for (rank, archive) in manifest.archives.iter().enumerate() {
        let package = TransactionPackage::WabbajackArchive { hash: archive.hash };
        let version = TransactionVersion::WabbajackHash { hash: archive.hash };
        let package_key = ModdeDependencyProvider::package_key(&package);
        let artifact = TransactionArtifact {
            package,
            version: version.clone(),
            display_name: archive.name.clone(),
            display_version: None,
            enabled_by_default: true,
            provenance: provenance.clone(),
        };
        provider.add_artifact(artifact, rank, Vec::new());
        requirements.push(provider.exact_requirement(&package_key, version));
    }

    let base_rank = manifest.archives.len();
    for (idx, directive) in manifest.install_directives().into_iter().enumerate() {
        match directive {
            InstallDirective::PatchedFromArchive {
                archive_hash,
                to,
                patch_id,
                ..
            } => {
                let package = TransactionPackage::WabbajackPatchOutput {
                    patch_id: patch_id.clone(),
                    output_hash: stable_output_hash(&to),
                };
                let version = TransactionVersion::WabbajackHash {
                    hash: stable_output_hash(&to),
                };
                let package_key = ModdeDependencyProvider::package_key(&package);
                let source_package =
                    ModdeDependencyProvider::package_key(&TransactionPackage::WabbajackArchive {
                        hash: archive_hash,
                    });
                let artifact = TransactionArtifact {
                    package,
                    version: version.clone(),
                    display_name: to,
                    display_version: None,
                    enabled_by_default: true,
                    provenance: TransactionProvenance::WabbajackPatch { patch_id },
                };
                provider.add_artifact(
                    artifact,
                    base_rank + idx,
                    vec![ExactRequirement {
                        package: source_package,
                        version: TransactionVersion::WabbajackHash { hash: archive_hash },
                    }],
                );
                requirements.push(provider.exact_requirement(&package_key, version));
            }
            InstallDirective::CreateBSA { temp_id, to, .. } => {
                let output_hash = stable_output_hash(&to);
                let package = TransactionPackage::WabbajackPatchOutput {
                    patch_id: format!("create-bsa:{temp_id}"),
                    output_hash,
                };
                let version = TransactionVersion::WabbajackHash { hash: output_hash };
                let package_key = ModdeDependencyProvider::package_key(&package);
                let artifact = TransactionArtifact {
                    package,
                    version: version.clone(),
                    display_name: to,
                    display_version: None,
                    enabled_by_default: true,
                    provenance: provenance.clone(),
                };
                provider.add_artifact(artifact, base_rank + idx, Vec::new());
                requirements.push(provider.exact_requirement(&package_key, version));
            }
            InstallDirective::FromArchive { .. } | InstallDirective::InlineFile { .. } => {}
        }
    }

    solve_transaction(provider, requirements)
}

fn solve_transaction(
    provider: ModdeDependencyProvider,
    requirements: Vec<ConditionalRequirement>,
) -> Result<TransactionPlan, TransactionError> {
    let mut solver = Solver::new(provider);
    let problem = Problem::new().requirements(requirements);
    let solvables = solver.solve(problem).map_err(|err| {
        if let Some(reason) = solver.provider().metadata_error() {
            return TransactionError::MetadataFetch { reason };
        }
        match err {
            resolvo::UnsolvableOrCancelled::Unsolvable(conflict) => {
                TransactionError::Unsatisfiable {
                    reason: conflict.display_user_friendly(&solver).to_string(),
                }
            }
            resolvo::UnsolvableOrCancelled::Cancelled(_) => TransactionError::Unsatisfiable {
                reason: "transaction solve cancelled".to_string(),
            },
        }
    })?;

    let mut artifacts = solvables
        .into_iter()
        .filter_map(|solvable| solver.provider().artifact_for(solvable))
        .collect::<Vec<_>>();
    artifacts.sort_by_key(|meta| meta.rank);
    Ok(TransactionPlan {
        artifacts: artifacts.into_iter().map(|meta| meta.artifact).collect(),
    })
}

fn solve_transaction_with_current_runtime(
    provider: ModdeDependencyProvider,
    requirements: Vec<ConditionalRequirement>,
) -> Result<TransactionPlan, TransactionError> {
    let handle =
        tokio::runtime::Handle::try_current().map_err(|error| TransactionError::MetadataFetch {
            reason: format!("lazy Nexus resolution requires an active Tokio runtime: {error}"),
        })?;
    let mut solver = Solver::new(provider).with_runtime(handle);
    let problem = Problem::new().requirements(requirements);
    let solvables = solver.solve(problem).map_err(|err| {
        if let Some(reason) = solver.provider().metadata_error() {
            return TransactionError::MetadataFetch { reason };
        }
        match err {
            resolvo::UnsolvableOrCancelled::Unsolvable(conflict) => {
                TransactionError::Unsatisfiable {
                    reason: conflict.display_user_friendly(&solver).to_string(),
                }
            }
            resolvo::UnsolvableOrCancelled::Cancelled(_) => TransactionError::Unsatisfiable {
                reason: "transaction solve cancelled".to_string(),
            },
        }
    })?;

    let mut artifacts = solvables
        .into_iter()
        .filter_map(|solvable| solver.provider().artifact_for(solvable))
        .collect::<Vec<_>>();
    artifacts.sort_by_key(|meta| meta.rank);
    Ok(TransactionPlan {
        artifacts: artifacts.into_iter().map(|meta| meta.artifact).collect(),
    })
}

fn stable_output_hash(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
