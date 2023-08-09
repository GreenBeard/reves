use std::collections::HashSet;
use std::path::PathBuf;

use camino::Utf8PathBuf;
use reves::DependencyKind;
use reves::OrphanArtifact;
use reves::OrphanArtifactKind;
use reves::UnusedDependency;

#[derive(Debug, Hash, Eq, PartialEq)]
struct ExpectedUnusedDependency {
    dependant: String,
    dependency: String,
    dep_kind: DependencyKind,
}

#[derive(Debug, Hash, Eq, PartialEq)]
struct ExpectedOrphanArtifact {
    crate_name: String,
    kind: OrphanArtifactKind,
    artifact_name: String,
    crate_relative_path: Utf8PathBuf,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TestStatus {
    Passing,
    /// Used to write test cases before they are passing.
    Todo,
}

struct Test {
    folder: Utf8PathBuf,
    test_status: TestStatus,
    expected_unused_dependencies: HashSet<ExpectedUnusedDependency>,
    expected_orphans: HashSet<ExpectedOrphanArtifact>,
}

fn package_id_to_name(pkg_id: &cargo_metadata::PackageId) -> &str {
    return &pkg_id.repr[0..pkg_id.repr.find(' ').unwrap()];
}

fn unused_dep_to_expected(unused_dep: &UnusedDependency) -> ExpectedUnusedDependency {
    return ExpectedUnusedDependency {
        dependant: package_id_to_name(&unused_dep.dependant).to_owned(),
        dependency: package_id_to_name(&unused_dep.dependency).to_owned(),
        dep_kind: unused_dep.dep_kind,
    };
}

fn equal_unused_deps(
    real_unused: &HashSet<UnusedDependency>,
    expected_unused: &HashSet<ExpectedUnusedDependency>,
) -> bool {
    if real_unused.len() == expected_unused.len() {
        for real_unused in real_unused.iter() {
            if !expected_unused.contains(&unused_dep_to_expected(real_unused)) {
                return false;
            }
        }
        return true;
    }
    return false;
}

fn orphan_artifact_to_expected(orphan: &OrphanArtifact) -> ExpectedOrphanArtifact {
    return ExpectedOrphanArtifact {
        crate_name: package_id_to_name(&orphan.crate_id).to_owned(),
        kind: orphan.kind.clone(),
        artifact_name: orphan.artifact_name.clone(),
        crate_relative_path: orphan.crate_relative_path.clone(),
    };
}

fn equal_orphan_artifacts(
    real_orphans: &HashSet<OrphanArtifact>,
    expected_orphans: &HashSet<ExpectedOrphanArtifact>,
) -> bool {
    if real_orphans.len() == expected_orphans.len() {
        for real_orphan in real_orphans.iter() {
            if !expected_orphans.contains(&orphan_artifact_to_expected(real_orphan)) {
                return false;
            }
        }
        return true;
    }
    return false;
}

fn main() {
    let tests: Vec<Test> = vec![
        Test {
            folder: Utf8PathBuf::from("link_dep"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::from_iter(vec![ExpectedUnusedDependency {
                dependant: "lily".to_owned(),
                dependency: "buttercup".to_owned(),
                dep_kind: DependencyKind::Normal,
            }]),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("link_dep_sometimes"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::from_iter(vec![ExpectedUnusedDependency {
                dependant: "lily".to_owned(),
                dependency: "buttercup".to_owned(),
                dep_kind: DependencyKind::Normal,
            }]),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("simple_unused"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::from_iter(vec![
                ExpectedUnusedDependency {
                    dependant: "magenta".to_owned(),
                    dependency: "fuchsia".to_owned(),
                    dep_kind: DependencyKind::Normal,
                },
                ExpectedUnusedDependency {
                    dependant: "magenta".to_owned(),
                    dependency: "purple".to_owned(),
                    dep_kind: DependencyKind::Development,
                },
                ExpectedUnusedDependency {
                    dependant: "magenta".to_owned(),
                    dependency: "fuchsia".to_owned(),
                    dep_kind: DependencyKind::Build,
                },
            ]),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("simple_used"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::new(),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("doc_test_used"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::from_iter(vec![ExpectedUnusedDependency {
                dependant: "lamb".to_owned(),
                dependency: "chick".to_owned(),
                dep_kind: DependencyKind::Development,
            }]),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("doc_test_ignore_used"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::from_iter(vec![ExpectedUnusedDependency {
                dependant: "lamb".to_owned(),
                dependency: "chick".to_owned(),
                dep_kind: DependencyKind::Development,
            }]),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("doc_broken_link"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::from_iter(vec![
                ExpectedUnusedDependency {
                    dependant: "lamb".to_owned(),
                    dependency: "bunny".to_owned(),
                    dep_kind: DependencyKind::Development,
                },
                ExpectedUnusedDependency {
                    dependant: "lamb".to_owned(),
                    dependency: "chick".to_owned(),
                    dep_kind: DependencyKind::Development,
                },
            ]),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("doc_working_link"),
            test_status: TestStatus::Todo,
            expected_unused_dependencies: HashSet::from_iter(vec![ExpectedUnusedDependency {
                dependant: "lamb".to_owned(),
                dependency: "chick".to_owned(),
                dep_kind: DependencyKind::Development,
            }]),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("rename_crates_unused"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::from_iter(vec![
                ExpectedUnusedDependency {
                    dependant: "magenta".to_owned(),
                    dependency: "fuchsia".to_owned(),
                    dep_kind: DependencyKind::Normal,
                },
                ExpectedUnusedDependency {
                    dependant: "magenta".to_owned(),
                    dependency: "purple".to_owned(),
                    dep_kind: DependencyKind::Development,
                },
                ExpectedUnusedDependency {
                    dependant: "magenta".to_owned(),
                    dependency: "fuchsia".to_owned(),
                    dep_kind: DependencyKind::Build,
                },
            ]),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("mislabeled_dev_dep"),
            test_status: TestStatus::Todo,
            expected_unused_dependencies: HashSet::from_iter(vec![ExpectedUnusedDependency {
                dependant: "wheat".to_owned(),
                dependency: "barley".to_owned(),
                dep_kind: DependencyKind::Normal,
            }]),
            expected_orphans: HashSet::new(),
        },
        Test {
            folder: Utf8PathBuf::from("orphans"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::new(),
            expected_orphans: HashSet::from_iter(vec![
                ExpectedOrphanArtifact {
                    crate_name: "pepper".to_owned(),
                    kind: OrphanArtifactKind::Binary,
                    artifact_name: "pepper".to_owned(),
                    crate_relative_path: Utf8PathBuf::from("src/main.rs"),
                },
                ExpectedOrphanArtifact {
                    crate_name: "pepper".to_owned(),
                    kind: OrphanArtifactKind::Binary,
                    artifact_name: "orphan_bin".to_owned(),
                    crate_relative_path: Utf8PathBuf::from("src/bin/orphan_bin.rs"),
                },
                ExpectedOrphanArtifact {
                    crate_name: "pepper".to_owned(),
                    kind: OrphanArtifactKind::Bench,
                    artifact_name: "orphan_bench".to_owned(),
                    crate_relative_path: Utf8PathBuf::from("benches/orphan_bench.rs"),
                },
                ExpectedOrphanArtifact {
                    crate_name: "pepper".to_owned(),
                    kind: OrphanArtifactKind::Test,
                    artifact_name: "orphan_test".to_owned(),
                    crate_relative_path: Utf8PathBuf::from("tests/orphan_test.rs"),
                },
                ExpectedOrphanArtifact {
                    crate_name: "pepper".to_owned(),
                    kind: OrphanArtifactKind::Example,
                    artifact_name: "orphan_example".to_owned(),
                    crate_relative_path: Utf8PathBuf::from("examples/orphan_example.rs"),
                },
            ]),
        },
        Test {
            folder: Utf8PathBuf::from("charges"),
            test_status: TestStatus::Passing,
            expected_unused_dependencies: HashSet::new(),
            expected_orphans: HashSet::new(),
        },
    ];

    let test_workspaces: PathBuf = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../test_workspaces");

    for test in tests.iter() {
        println!("Executing test case {}", test.folder);

        if let Ok(lint_results) = reves::lint_dependencies(
            test_workspaces.join(test.folder.as_path()).as_path(),
            true,
            &reves::CargoArgs {
                color: clap::ColorChoice::Auto,
                frozen: false,
                locked: false,
                offline: true,
                workspace: true,
                config: Vec::new(),
                target_dir: None,
                manifest_path: None,
            },
        ) {
            if !equal_unused_deps(
                &lint_results.unused_dependencies,
                &test.expected_unused_dependencies,
            ) || !equal_orphan_artifacts(&lint_results.orphans, &test.expected_orphans)
            {
                match test.test_status {
                    TestStatus::Passing => {
                        println!("Failing test case results {}", test.folder);
                    }
                    TestStatus::Todo => {
                        // An expected failure
                    }
                }
            } else {
                match test.test_status {
                    TestStatus::Passing => {
                        // An expected success
                    }
                    TestStatus::Todo => {
                        println!("No longer todo test case {}", test.folder);
                    }
                }
            }
        } else {
            match test.test_status {
                TestStatus::Passing => {
                    println!("Failing test case execution {}", test.folder);
                }
                TestStatus::Todo => {
                    // An expected failure
                }
            }
        }
    }
}
