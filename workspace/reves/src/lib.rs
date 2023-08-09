// TODO: properly clear environment variable for subcommands.

/*
  This crate attempts to use the phrase "artifact" everywhere that cargo uses
  "target" for the --all-targets kind (as opposed to the --target kind).

  The goal of this crate is to find (and automatically remove with `--fix`)
  unused dependencies. Due to the complicated process by which cargo figures out
  what artifacts to build for each crate, and how cargo decides which crates to
  provide to the artifacts that it is building this is tool doesn't attempt to
  support specifying which artifacts to build.

  Courtesy of est31 (with corrections) https://github.com/rust-lang/cargo/pull/8437#issuecomment-653842297

  Command \ Artifacts   lib.rs   main.rs   cfg(test) tests/   benches/   doctests
 --------------------- -------- --------- ------------------ ---------- ----------
  check                 yes      yes       no                 no         no
  check --all-targets   yes      yes       yes                yes        no
  test --no-run         yes      yes       yes                no         no
  test                  yes      yes       yes                no         yes
  test --all-targets    yes      yes       yes                yes        no

*/

use std::borrow::Borrow;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::str::FromStr;

use anyhow::Context;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use cargo_metadata::semver;
use regex::Regex;

mod cargo_links;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
struct UnusedExterns {
    lint_level: String,
    unused_extern_names: Vec<String>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct UnrenamedCrate<'a> {
    name: Cow<'a, str>,
}

type UnrenamedCrateOwned = UnrenamedCrate<'static>;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RenamedCrate<'a> {
    name: Cow<'a, str>,
}

type RenamedCrateOwned = RenamedCrate<'static>;

impl<'a> RenamedCrate<'a> {
    fn as_unowned(&'a self) -> RenamedCrate<'a> {
        return RenamedCrate {
            name: Cow::Borrowed(self.name.borrow()),
        };
    }
}

impl<'a> UnrenamedCrate<'a> {
    fn _as_unowned(&'a self) -> UnrenamedCrate<'a> {
        return UnrenamedCrate {
            name: Cow::Borrowed(self.name.borrow()),
        };
    }
}

/// `cargo` is unusual in handling host vs target dependencies at the moment
/// (until the `host-config` feature is stabilized). This crate attempts to
/// avoid using any unstable `cargo` features; as such, build scripts cannot
/// currently be checked for unused dependencies when the target is not the host
/// which is really really dumb but rarely an issue in practice.
#[allow(dead_code)]
enum CheckTarget {
    /// Runs `cargo` without passing `--target`
    Host,
    /// Runs `cargo` with `--target`
    Target(String),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DependencyKind {
    Normal,
    Development,
    Build,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UnusedDependency {
    pub dependant: cargo_metadata::PackageId,
    pub dependency: cargo_metadata::PackageId,
    pub dep_kind: DependencyKind,

    dependency_name: UnrenamedCrateOwned,
    dependant_manifest_path: Utf8PathBuf,
}

pub struct DependencyLintResults {
    // Dependencies that appear to be removable based upon the currently
    // selected features, and target.
    pub unused_dependencies: HashSet<UnusedDependency>,
    // TODO: add information for "dependencies" that could be downgraded to
    // being a regular "dependencies".
    pub mismarked_dev_dependencies: (),
    // Artifacts that could have no dependency upon their associated crate
    // library.
    pub orphans: HashSet<OrphanArtifact>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum OrphanArtifactKind {
    Bench,
    Binary,
    Example,
    Test,
}

// Intentionally slightly lossy
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ArtifactKind {
    Bench,
    Binary,
    BuildScript,
    Example,
    Library,
    Test,
}

impl std::convert::TryFrom<ArtifactKind> for OrphanArtifactKind {
    type Error = anyhow::Error;

    fn try_from(value: ArtifactKind) -> anyhow::Result<OrphanArtifactKind> {
        return match value {
            ArtifactKind::Bench => Ok(OrphanArtifactKind::Bench),
            ArtifactKind::Binary => Ok(OrphanArtifactKind::Binary),
            ArtifactKind::Example => Ok(OrphanArtifactKind::Example),
            ArtifactKind::Test => Ok(OrphanArtifactKind::Test),
            _ => anyhow::bail!("unexpected orphan artifact kind"),
        };
    }
}

fn kind_to_artifact_kind(kind_strings: &[String]) -> anyhow::Result<ArtifactKind> {
    let mut flattened_artifact_kind: Option<ArtifactKind> = None;
    for kind_string in kind_strings.iter() {
        let artifact_kind: ArtifactKind = match kind_string.as_str() {
            "custom-build" => ArtifactKind::BuildScript,
            "bench" => ArtifactKind::Bench,
            "bin" => ArtifactKind::Binary,
            "example" => ArtifactKind::Example,
            "test" => ArtifactKind::Test,
            "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro" => {
                ArtifactKind::Library
            }
            _ => {
                anyhow::bail!("unsupported artifact kind {}", kind_string);
            }
        };
        if let Some(flattened_artifact_kind) = flattened_artifact_kind {
            if flattened_artifact_kind != artifact_kind {
                anyhow::bail!("mismatched artifact kinds");
            }
        }
        flattened_artifact_kind = Some(artifact_kind);
    }
    return flattened_artifact_kind.context("missing artifact kind");
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OrphanArtifact {
    pub crate_id: cargo_metadata::PackageId,
    pub kind: OrphanArtifactKind,
    pub artifact_name: String,
    pub crate_relative_path: Utf8PathBuf,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct UsedLinkDependency {
    pub dependant: cargo_metadata::PackageId,
    pub dependency: cargo_metadata::PackageId,
}

#[derive(Clone, Copy, Eq, PartialEq)]
#[allow(dead_code)]
enum CargoInvocationKind {
    /// `check --all-targets`
    CheckAllArtifacts,
    /// `test --doc`
    Doc,
}

#[allow(dead_code)]
enum Features {
    Specified(Vec<String>),
    Default,
    All,
}

fn parse_cargo_version_output(output: &str) -> anyhow::Result<semver::Version> {
    let release_regex = Regex::new("^release:(.*)$").unwrap();
    let mut release: Option<semver::Version> = None;
    for line in output.lines() {
        if let Some(captures) = release_regex.captures(line) {
            anyhow::ensure!(release.is_none(), "multiple cargo versions found");
            release = Some(semver::Version::parse(captures[1].trim())?);
        }
    }

    return release.context("unable to find cargo version");
}

fn cargo_version(workspace: &Path) -> anyhow::Result<semver::Version> {
    let output: std::process::Output = Command::new(cargo_command())
        .current_dir(workspace)
        .args(["-v", "--version"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()?;

    return parse_cargo_version_output(std::str::from_utf8(output.stdout.as_slice())?);
}

fn supports_default_workspace_members(cargo_version: &semver::Version) -> bool {
    return semver::Comparator::parse(">=1.71")
        .unwrap()
        .matches(cargo_version);
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum WorkspaceMembers {
    Default,
    All,
}

impl WorkspaceMembers {
    fn from_workspace_arg(workspace: bool) -> WorkspaceMembers {
        if workspace {
            return WorkspaceMembers::All;
        } else {
            return WorkspaceMembers::Default;
        }
    }
}

fn workspace_members(
    structured_metadata: &StructuredMetadata,
    members: WorkspaceMembers,
) -> &HashSet<cargo_metadata::PackageId> {
    return match members {
        WorkspaceMembers::Default => structured_metadata
            .default_workspace_members
            .as_ref()
            .unwrap(),
        WorkspaceMembers::All => &structured_metadata.all_workspace_members,
    };
}

fn toml_key_to_dep_kind(key: &str) -> Option<DependencyKind> {
    return match key {
        "dependencies" => Some(DependencyKind::Normal),
        "dev-dependencies" => Some(DependencyKind::Development),
        "build-dependencies" => Some(DependencyKind::Build),
        _ => None,
    };
}

fn cargo_command() -> Cow<'static, OsStr> {
    return match std::env::var_os("CARGO") {
        Some(cargo_command) => Cow::Owned(cargo_command),
        None => Cow::Borrowed(OsStr::new("cargo")),
    };
}

/// Returns the renamed version of the crate
fn parse_unused_crate_diagnostic(message: &str) -> anyhow::Result<RenamedCrateOwned> {
    let re = Regex::new("^external crate `([^`]*)` unused in `[^`]*`.*$").unwrap();
    let captures: regex::Captures = re
        .captures(message)
        .context("Unable to parse unused_crate_dependencies diagnostic message")?;
    return Ok(RenamedCrateOwned {
        name: Cow::Owned(captures[1].to_owned()),
    });
}

struct MetadataNode {
    deps: HashMap<RenamedCrateOwned, cargo_metadata::NodeDep>,
}

struct StructuredMetadata {
    nodes: HashMap<cargo_metadata::PackageId, MetadataNode>,
    packages: HashMap<cargo_metadata::PackageId, cargo_metadata::Package>,
    all_workspace_members: HashSet<cargo_metadata::PackageId>,
    default_workspace_members: Option<HashSet<cargo_metadata::PackageId>>,
    crate_links: BTreeMap<String, cargo_metadata::PackageId>,
}

fn metadata_to_structured_metadata(
    metadata: &cargo_metadata::Metadata,
    cargo_version: &semver::Version,
) -> anyhow::Result<StructuredMetadata> {
    let resolve: &cargo_metadata::Resolve = metadata
        .resolve
        .as_ref()
        .context("Missing cargo_metadata resolve")?;
    let mut nodes =
        HashMap::<cargo_metadata::PackageId, MetadataNode>::with_capacity(resolve.nodes.len());
    for node in resolve.nodes.iter() {
        let mut deps =
            HashMap::<RenamedCrateOwned, cargo_metadata::NodeDep>::with_capacity(node.deps.len());
        for dep in node.deps.iter() {
            let old_value: Option<_> = deps.insert(
                RenamedCrateOwned {
                    name: Cow::Owned(dep.name.clone()),
                },
                dep.clone(),
            );
            anyhow::ensure!(old_value.is_none());
        }
        let old_value: Option<_> = nodes.insert(node.id.clone(), MetadataNode { deps });
        anyhow::ensure!(old_value.is_none());
    }

    let mut packages = HashMap::<cargo_metadata::PackageId, cargo_metadata::Package>::with_capacity(
        metadata.packages.len(),
    );
    for package in metadata.packages.iter() {
        let old_value: Option<_> = packages.insert(package.id.clone(), package.clone());
        anyhow::ensure!(old_value.is_none());
    }

    let mut all_workspace_members =
        HashSet::<cargo_metadata::PackageId>::with_capacity(metadata.workspace_members.len());
    for workspace_member in metadata.workspace_members.iter() {
        let is_new: bool = all_workspace_members.insert(workspace_member.clone());
        anyhow::ensure!(is_new);
    }

    let default_workspace_members: Option<HashSet<cargo_metadata::PackageId>> =
        if supports_default_workspace_members(cargo_version) {
            let mut default_workspace_members = HashSet::<cargo_metadata::PackageId>::with_capacity(
                metadata.workspace_members.len(),
            );
            for workspace_member in metadata.workspace_default_members.iter() {
                let is_new: bool = default_workspace_members.insert(workspace_member.clone());
                anyhow::ensure!(is_new);
            }
            anyhow::ensure!(default_workspace_members
                .difference(&all_workspace_members)
                .next()
                .is_none());
            Some(default_workspace_members)
        } else {
            None
        };

    let mut crate_links = BTreeMap::<String, cargo_metadata::PackageId>::new();
    for package in metadata.packages.iter() {
        if let Some(link) = package.links.as_ref() {
            let old_value: Option<_> =
                crate_links.insert(cargo_links::envify(link), package.id.clone());
            anyhow::ensure!(old_value.is_none());
        }
    }

    return Ok(StructuredMetadata {
        nodes,
        packages,
        all_workspace_members,
        default_workspace_members,
        crate_links,
    });
}

// If a dependency is specified in multiple ways then it may be listed multiple
// times (such as one way is under a cfg(...) target). Therefore we use HashSet
// to deduplicate these.
fn dependency_kinds(dep: &cargo_metadata::NodeDep) -> anyhow::Result<HashSet<DependencyKind>> {
    assert!(!dep.dep_kinds.is_empty());
    let mut s = HashSet::<DependencyKind>::new();

    for dep_kind in dep.dep_kinds.iter() {
        match dep_kind.kind {
            cargo_metadata::DependencyKind::Normal => {
                s.insert(DependencyKind::Normal);
            }
            cargo_metadata::DependencyKind::Development => {
                s.insert(DependencyKind::Development);
            }
            cargo_metadata::DependencyKind::Build => {
                s.insert(DependencyKind::Build);
            }
            _ => {
                anyhow::bail!("Unsupported dependency kind {}", dep_kind.kind);
            }
        }
    }

    return Ok(s);
}

fn find_node_dep<'a>(
    krate: RenamedCrate<'a>,
    metadata_node: &'a MetadataNode,
) -> anyhow::Result<&'a cargo_metadata::NodeDep> {
    return metadata_node
        .deps
        .get(&krate)
        .context(format!("Missing crate {} in NodeDep list", krate.name));
}

/*
  Definitely suboptimal, and we could transform the `Vec` into a `HashMap` at a
  higher level, but this should be fine in most set ups.
*/
fn find_package_dependency<'a>(
    krate: UnrenamedCrate<'a>,
    deps: &'a [cargo_metadata::Dependency],
) -> anyhow::Result<&'a cargo_metadata::Dependency> {
    for dep in deps.iter() {
        if dep.name == krate.name {
            return Ok(dep);
        }
    }
    anyhow::bail!("Missing crate in Dependency list");
}

fn has_lib_artifact(artifacts: &[cargo_metadata::Target]) -> anyhow::Result<bool> {
    for artifact in artifacts.iter() {
        if kind_to_artifact_kind(&artifact.kind)? == ArtifactKind::Library {
            return Ok(true);
        }
    }
    return Ok(false);
}

fn compute_cargo_args(cargo_args: &CargoArgs) -> Vec<Cow<'_, OsStr>> {
    let mut args = Vec::<Cow<'static, OsStr>>::new();
    args.push(Cow::Borrowed(OsStr::new("--color")));
    args.push(Cow::Owned(OsString::from(format!("{}", cargo_args.color))));
    if cargo_args.frozen {
        args.push(Cow::Borrowed(OsStr::new("--frozen")));
    }
    if cargo_args.locked {
        args.push(Cow::Borrowed(OsStr::new("--locked")));
    }
    if cargo_args.offline {
        args.push(Cow::Borrowed(OsStr::new("--offline")));
    }
    if cargo_args.workspace {
        args.push(Cow::Borrowed(OsStr::new("--workspace")));
    }
    for config in cargo_args.config.iter() {
        args.push(Cow::Borrowed(OsStr::new("--config")));
        args.push(Cow::Borrowed(OsStr::new(config.as_str())));
    }
    if let Some(target_dir) = cargo_args.target_dir.as_ref() {
        args.push(Cow::Borrowed(OsStr::new("--target-dir")));
        args.push(Cow::Borrowed(target_dir.as_os_str()));
    }
    if let Some(manifest_path) = cargo_args.manifest_path.as_ref() {
        args.push(Cow::Borrowed(OsStr::new("--manifest-path")));
        args.push(Cow::Borrowed(manifest_path.as_os_str()));
    }
    return args;
}

fn compute_feature_args(features: &Features) -> Vec<Cow<'static, OsStr>> {
    let mut args = Vec::<Cow<'static, OsStr>>::new();
    match features {
        Features::Specified(features) => {
            args.push(Cow::Borrowed(OsStr::new("--features")));

            let mut features_len: usize = 0;
            /* +1, and -1 for commas */
            for feature in features.iter() {
                features_len += feature.len() + 1;
            }
            if features_len > 0 {
                features_len -= 1;
            }

            let mut joined_features = OsString::with_capacity(features_len);
            for i in 0..features.len() {
                if i != 0 {
                    joined_features.push(OsStr::new(","));
                }
                joined_features.push(OsStr::new(features[i].as_str()));
            }

            args.push(Cow::Borrowed(OsStr::new("--features")));
            args.push(Cow::Owned(joined_features));
        }
        Features::Default => { /* do nothing */ }
        Features::All => {
            args.push(Cow::Borrowed(OsStr::new("--all-features")));
        }
    }
    return args;
}

fn compute_target_args(check_target: &CheckTarget) -> Vec<Cow<'_, OsStr>> {
    let mut args = Vec::<Cow<'static, OsStr>>::new();
    match check_target {
        CheckTarget::Host => { /* do nothing */ }
        CheckTarget::Target(target) => {
            args.push(Cow::Borrowed(OsStr::new("--target")));
            args.push(Cow::Borrowed(OsStr::new(target)));
        }
    }
    return args;
}

fn compute_encoded_flags(flags: &[&str]) -> String {
    let mut flag_string_size: usize = 0;
    for i in 0..flags.len() {
        assert!(!flags[i].contains('\u{1f}'));
        flag_string_size += flags[i].len() + if i != 0 { 1 } else { 0 };
    }
    let mut flag_string = String::with_capacity(flag_string_size);
    for i in 0..flags.len() {
        if i != 0 {
            flag_string.push('\u{1f}');
        }
        flag_string.push_str(flags[i]);
    }
    assert!(flag_string.len() == flag_string_size);
    return flag_string;
}

fn find_unused_dependencies_doc(
    workspace: &Path,
    check_target: &CheckTarget,
    features: &Features,
    structured_metadata: &StructuredMetadata,
    cargo_args: &CargoArgs,
) -> anyhow::Result<HashSet<UnusedDependency>> {
    let mut unused_deps = HashSet::<UnusedDependency>::new();

    let mut args = Vec::<Cow<'static, OsStr>>::new();
    let mut rustdoctest_args = Vec::<Cow<'static, OsStr>>::new();
    let mut env = HashMap::<Cow<'static, OsStr>, Cow<'static, OsStr>>::new();

    args.push(Cow::Borrowed(OsStr::new("test")));
    //args.append(&mut compute_cargo_args(cargo_args));
    {
        args.push(Cow::Borrowed(OsStr::new("--color")));
        args.push(Cow::Owned(OsString::from(format!("{}", cargo_args.color))));
        if cargo_args.frozen {
            args.push(Cow::Borrowed(OsStr::new("--frozen")));
        }
        if cargo_args.locked {
            args.push(Cow::Borrowed(OsStr::new("--locked")));
        }
        if cargo_args.offline {
            args.push(Cow::Borrowed(OsStr::new("--offline")));
        }
        // TODO fix
        /*if cargo_args.workspace {
            args.push(Cow::Borrowed(OsStr::new("--workspace")));
        }*/
        for config in cargo_args.config.iter() {
            args.push(Cow::Borrowed(OsStr::new("--config")));
            args.push(Cow::Borrowed(OsStr::new(config.as_str())));
        }
        if let Some(target_dir) = cargo_args.target_dir.as_ref() {
            args.push(Cow::Borrowed(OsStr::new("--target-dir")));
            args.push(Cow::Borrowed(target_dir.as_os_str()));
        }
        if let Some(manifest_path) = cargo_args.manifest_path.as_ref() {
            args.push(Cow::Borrowed(OsStr::new("--manifest-path")));
            args.push(Cow::Borrowed(manifest_path.as_os_str()));
        }
    }
    args.push(Cow::Borrowed(OsStr::new("--quiet")));
    args.push(Cow::Borrowed(OsStr::new("--doc")));
    args.push(Cow::Borrowed(OsStr::new("--target-dir=target_reves_doc")));
    args.push(Cow::Borrowed(OsStr::new("--message-format=json")));

    args.append(&mut compute_target_args(check_target));
    args.append(&mut compute_feature_args(features));

    // --json=unused-externs-silent only works if running all tests (including
    // ignored). If this causes the crate to not compile then `ignored` should be
    // switched to `text`.
    rustdoctest_args.push(Cow::Borrowed(OsStr::new("--include-ignored")));

    // TODO: remove. Just for testing purposes
    env.insert(
        Cow::Borrowed(OsStr::new("RUSTC_BOOTSTRAP")),
        Cow::Borrowed(OsStr::new("1")),
    );

    env.insert(
        Cow::Borrowed(OsStr::new("CARGO_ENCODED_RUSTDOCFLAGS")),
        Cow::Owned(OsString::from(compute_encoded_flags(&[
            "--json=unused-externs-silent",
            "--warn=unused-crate-dependencies",
            "--no-run",
            "-Z",
            "unstable-options",
        ]))),
    );

    for package_id in workspace_members(
        structured_metadata,
        WorkspaceMembers::from_workspace_arg(cargo_args.workspace),
    ) {
        if !has_lib_artifact(structured_metadata.packages[&package_id].targets.as_slice())? {
            // Skip, only "lib"s have doc tests.
            continue;
        }

        let output: std::process::Output = Command::new(cargo_command())
            .current_dir(workspace)
            .args(&args)
            .args([
                "-p",
                structured_metadata.packages[&package_id].name.as_str(),
            ])
            .arg("--")
            .args(&rustdoctest_args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .envs(env.clone())
            .output()?;
        anyhow::ensure!(output.status.success());

        let unused_externs: UnusedExterns =
            serde_json::from_str(std::str::from_utf8(output.stderr.as_slice())?)?;

        for unused_extern in unused_externs.unused_extern_names.iter() {
            let renamed_crate = RenamedCrate {
                name: Cow::Borrowed(unused_extern.as_str()),
            };
            match find_node_dep(
                renamed_crate.clone(),
                &structured_metadata.nodes[&package_id],
            ) {
                Ok(node_dep) => {
                    let krate: UnrenamedCrateOwned = UnrenamedCrateOwned {
                        name: Cow::Owned(
                            structured_metadata.packages[&node_dep.pkg].name.to_owned(),
                        ),
                    };
                    let dependency: &cargo_metadata::Dependency = find_package_dependency(
                        krate,
                        structured_metadata.packages[&package_id]
                            .dependencies
                            .as_slice(),
                    )?;
                    for dep_kind in dependency_kinds(node_dep)?.into_iter() {
                        let unused_dep = UnusedDependency {
                            dependant: package_id.clone(),
                            dependency: node_dep.pkg.clone(),
                            dep_kind,

                            dependency_name: UnrenamedCrateOwned {
                                name: Cow::Owned(dependency.name.clone()),
                            },
                            dependant_manifest_path: structured_metadata.packages[&package_id]
                                .manifest_path
                                .clone(),
                        };
                        let is_new: bool = unused_deps.insert(unused_dep.clone());
                        assert!(is_new, "{:#?}", unused_dep);
                    }
                }
                Err(e) => {
                    /*
                      A crate can't rename itself so they should be equivalent other
                      than regular name normalization if the crate is referring to
                      itself as a library (note that the crate binaries, and
                      examples may have different names than the crate library so
                      the target name isn't the same as the crate name).
                    */
                    if renamed_crate.name
                        != structured_metadata.packages[&package_id]
                            .name
                            .replace('-', "_")
                            .as_str()
                    {
                        return Err(e);
                    } else {
                        /*
                          Ignore unused self-reference. For example, binary crate not
                          using its own library crate. TODO: decide what to do.
                        */
                    }
                }
            }
        }
    }

    return Ok(unused_deps);
}

pub struct CargoArgs {
    pub color: clap::ColorChoice,
    pub frozen: bool,
    pub locked: bool,
    pub offline: bool,
    pub workspace: bool,
    pub config: Vec<String>,
    pub target_dir: Option<PathBuf>,
    pub manifest_path: Option<PathBuf>,
}

fn find_unused_dependencies_check(
    workspace: &Path,
    check_target: &CheckTarget,
    features: &Features,
    structured_metadata: &StructuredMetadata,
    cargo_args: &CargoArgs,
) -> anyhow::Result<DependencyLintResults> {
    let mut package_artifacts =
        HashMap::<cargo_metadata::PackageId, HashSet<cargo_metadata::Artifact>>::new();
    let mut unused_deps = HashMap::<UnusedDependency, HashSet<cargo_metadata::Artifact>>::new();
    let mut all_link_deps = HashSet::<UsedLinkDependency>::new();
    let mut orphans = HashSet::<OrphanArtifact>::new();

    let mut args = Vec::<Cow<'static, OsStr>>::new();
    let mut env = HashMap::<Cow<'static, OsStr>, Cow<'static, OsStr>>::new();

    args.push(Cow::Borrowed(OsStr::new("check")));
    args.append(&mut compute_cargo_args(cargo_args));
    args.push(Cow::Borrowed(OsStr::new("--all-targets")));
    args.push(Cow::Borrowed(OsStr::new("--target-dir=target_reves")));
    args.push(Cow::Borrowed(OsStr::new("--message-format=json")));

    args.append(&mut compute_target_args(check_target));
    args.append(&mut compute_feature_args(features));

    env.insert(
        Cow::Borrowed(OsStr::new("CARGO_ENCODED_RUSTFLAGS")),
        Cow::Owned(OsString::from(compute_encoded_flags(&[
            "--warn=unused-crate-dependencies",
        ]))),
    );

    let status = Command::new(cargo_command())
        .current_dir(workspace)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .envs(env.clone())
        .status()?;
    anyhow::ensure!(status.success());

    args.push(Cow::Borrowed(OsStr::new("-j1")));
    let mut command = Command::new(cargo_command())
        .current_dir(workspace)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .envs(env)
        .spawn()?;

    let reader = std::io::BufReader::new(command.stdout.take().unwrap());
    let mut active_message_artifact: Option<cargo_metadata::Target> = None;
    let mut active_unused_deps = Vec::<UnusedDependency>::new();
    for message in cargo_metadata::Message::parse_stream(reader) {
        match message? {
            cargo_metadata::Message::CompilerArtifact(artifact) => {
                if let Some(artifacts) = package_artifacts.get_mut(&artifact.package_id) {
                    artifacts.insert(artifact.clone());
                } else {
                    package_artifacts.insert(
                        artifact.package_id.clone(),
                        HashSet::from_iter([artifact.clone()]),
                    );
                }
                if let Some(message_artifact) = active_message_artifact.take() {
                    assert!(message_artifact == artifact.target);
                    for unused_dep in active_unused_deps.drain(..) {
                        /*
                          Can do the check here as opposed to at the end as build scripts
                          always run before the rest of the crate work.
                        */
                        if unused_dep.dep_kind == DependencyKind::Normal
                            && all_link_deps.contains(&UsedLinkDependency {
                                dependant: unused_dep.dependant.clone(),
                                dependency: unused_dep.dependency.clone(),
                            })
                        {
                            /* used as a link dep */
                            continue;
                        }

                        if let Some(artifacts) = unused_deps.get_mut(&unused_dep) {
                            artifacts.insert(artifact.clone());
                        } else {
                            unused_deps.insert(unused_dep, HashSet::from_iter([artifact.clone()]));
                        }
                    }
                } else {
                    assert!(active_unused_deps.is_empty());
                }
            }
            cargo_metadata::Message::CompilerMessage(message) => {
                if structured_metadata
                    .all_workspace_members
                    .contains(&message.package_id)
                {
                    if let Some(message_artifact) = active_message_artifact.as_ref() {
                        assert!(*message_artifact == message.target);
                    } else {
                        active_message_artifact = Some(message.target.clone());
                    }
                    if let Some(diagnostic_code) = &message.message.code {
                        if diagnostic_code.code.as_str() == "unused_crate_dependencies" {
                            let renamed_crate: RenamedCrateOwned =
                                parse_unused_crate_diagnostic(message.message.message.as_str())?;
                            match find_node_dep(
                                renamed_crate.as_unowned(),
                                &structured_metadata.nodes[&message.package_id],
                            ) {
                                Ok(node_dep) => {
                                    let krate: UnrenamedCrateOwned = UnrenamedCrateOwned {
                                        name: Cow::Owned(
                                            structured_metadata.packages[&node_dep.pkg]
                                                .name
                                                .to_owned(),
                                        ),
                                    };
                                    let dependency: &cargo_metadata::Dependency =
                                        find_package_dependency(
                                            krate,
                                            structured_metadata.packages[&message.package_id]
                                                .dependencies
                                                .as_slice(),
                                        )?;
                                    for dep_kind in dependency_kinds(node_dep)?.into_iter() {
                                        let unused_dep = UnusedDependency {
                                            dependant: message.package_id.clone(),
                                            dependency: node_dep.pkg.clone(),
                                            dep_kind,

                                            dependency_name: UnrenamedCrateOwned {
                                                name: Cow::Owned(dependency.name.clone()),
                                            },
                                            dependant_manifest_path: structured_metadata.packages
                                                [&message.package_id]
                                                .manifest_path
                                                .clone(),
                                        };
                                        active_unused_deps.push(unused_dep);
                                    }
                                }
                                Err(e) => {
                                    /*
                                      A crate can't rename itself so they should be equivalent other
                                      than regular name normalization if the crate is referring to
                                      itself as a library (note that the crate binaries, and
                                      examples may have different names than the crate library so
                                      the artifact name isn't the same as the crate name).
                                    */
                                    if <Cow<'_, str> as Borrow<str>>::borrow(&renamed_crate.name)
                                        != structured_metadata.packages[&message.package_id]
                                            .name
                                            .replace('-', "_")
                                            .as_str()
                                    {
                                        return Err(e);
                                    } else {
                                        orphans.insert(OrphanArtifact {
                                            crate_id: message.package_id.clone(),
                                            kind: OrphanArtifactKind::try_from(
                                                kind_to_artifact_kind(&message.target.kind)?,
                                            )?,
                                            artifact_name: message.target.name.clone(),
                                            crate_relative_path: message
                                                .target
                                                .src_path
                                                .strip_prefix(
                                                    structured_metadata.packages
                                                        [&message.package_id]
                                                        .manifest_path
                                                        .parent()
                                                        .unwrap(),
                                                )
                                                .unwrap()
                                                .to_owned(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            cargo_metadata::Message::BuildScriptExecuted(build_script_info) => {
                assert!(active_message_artifact.is_none());
                let mut out_txt_path: Utf8PathBuf = build_script_info.out_dir.clone();
                out_txt_path.pop();
                out_txt_path.push("output");

                let out_txt_read: BufReader<_> =
                    BufReader::new(File::open(out_txt_path.as_path())?);
                for line in out_txt_read.lines() {
                    if let Some((prefixed_key, value)) = line?.split_once('=') {
                        if let Some(key) = prefixed_key.strip_prefix("cargo:") {
                            if key == "rerun-if-env-changed" {
                                // The code as written here may have false positives (if a
                                // crate declares a DEP_ usage, but doesn't actually have it
                                // as a dependency) but that is fine for the purposes of this
                                // code.
                                if let Some(link_var) = value.strip_prefix("DEP_") {
                                    match cargo_links::find_crate(
                                        link_var,
                                        &structured_metadata.crate_links,
                                    ) {
                                        Ok(provider) => {
                                            all_link_deps.insert(UsedLinkDependency {
                                                dependant: build_script_info.package_id.clone(),
                                                dependency: provider.clone(),
                                            });
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "Warning: Provider of link var DEP_{} used by {} not found - {}",
                                                link_var,
                                                build_script_info.package_id,
                                                e,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            cargo_metadata::Message::BuildFinished(_) => {
                assert!(active_message_artifact.is_none());
                /* don't care */
            }
            cargo_metadata::Message::TextLine(_) => {
                anyhow::bail!("Unexpected text line");
            }
            _ => { /* do nothing, some message type we don't care about */ }
        }
    }

    assert!(active_message_artifact.is_none());
    assert!(active_unused_deps.is_empty());

    let status: ExitStatus = command.wait()?;
    anyhow::ensure!(status.success());

    // Ensure that all artifacts didn't use it before reporting an issue

    // UnusedDependency is true for all artifacts built that may have been able to
    // use it.
    let mut unused_deps_squashed = HashSet::<UnusedDependency>::new();
    for (unused_dep, artifacts) in unused_deps.iter() {
        let mut possible_users = HashSet::<cargo_metadata::Artifact>::new();

        for artifact in package_artifacts[&unused_dep.dependant].iter() {
            match kind_to_artifact_kind(&artifact.target.kind)? {
                ArtifactKind::Binary | ArtifactKind::Library => {
                    match unused_dep.dep_kind {
                        DependencyKind::Normal => {
                            possible_users.insert(artifact.clone());
                        }
                        DependencyKind::Development => {
                            if artifact.profile.test {
                                possible_users.insert(artifact.clone());
                            }
                        }
                        DependencyKind::Build => { /* can't use it */ }
                    }
                }
                ArtifactKind::Bench | ArtifactKind::Example | ArtifactKind::Test => {
                    match unused_dep.dep_kind {
                        DependencyKind::Normal | DependencyKind::Development => {
                            possible_users.insert(artifact.clone());
                        }
                        DependencyKind::Build => { /* can't use it */ }
                    }
                }
                ArtifactKind::BuildScript => {
                    match unused_dep.dep_kind {
                        DependencyKind::Build => {
                            possible_users.insert(artifact.clone());
                        }
                        DependencyKind::Normal | DependencyKind::Development => {
                            /* can't use it */
                        }
                    }
                }
            }
        }

        if artifacts.difference(&possible_users).next().is_some() {
            // Bug in this code as artifacts should always be a subset of
            // possible_users unless the `dep_kinds` field of
            // `cargo_metadata::NodeDep` was ambiguous. TODO: readd some sanity
            // checking somewhere.
            //unreachable!();
        }
        if possible_users.difference(artifacts).next().is_none() {
            unused_deps_squashed.insert(unused_dep.clone());
        }
    }

    return Ok(DependencyLintResults {
        unused_dependencies: unused_deps_squashed,
        mismarked_dev_dependencies: (),
        orphans,
    });
}

fn find_unused_dependencies_all_invocations(
    workspace: &Path,
    check_target: &CheckTarget,
    features: &Features,
    structured_metadata: &StructuredMetadata,
    check_doc_tests: bool,
    cargo_args: &CargoArgs,
) -> anyhow::Result<DependencyLintResults> {
    let regular_lint_results: DependencyLintResults = find_unused_dependencies_check(
        workspace,
        check_target,
        features,
        structured_metadata,
        cargo_args,
    )?;
    let doc_unused_deps: Option<HashSet<UnusedDependency>> = if check_doc_tests {
        Some(find_unused_dependencies_doc(
            workspace,
            check_target,
            features,
            structured_metadata,
            cargo_args,
        )?)
    } else {
        None
    };

    let mut combined_unused_deps: HashSet<UnusedDependency> = HashSet::new();
    for dep in regular_lint_results.unused_dependencies.into_iter() {
        match dep.dep_kind {
            DependencyKind::Normal | DependencyKind::Development => {
                if let Some(doc_unused_deps) = doc_unused_deps.as_ref() {
                    if doc_unused_deps.contains(&dep) {
                        combined_unused_deps.insert(dep);
                    }
                } else {
                    combined_unused_deps.insert(dep);
                }
            }
            DependencyKind::Build => {
                combined_unused_deps.insert(dep);
            }
        }
    }

    return Ok(DependencyLintResults {
        unused_dependencies: combined_unused_deps,
        mismarked_dev_dependencies: (),
        orphans: regular_lint_results.orphans,
    });
}

#[derive(clap::Parser)]
pub struct Args {
    #[arg(long, default_value_t)]
    color: clap::ColorChoice,

    /// Passed to `cargo` invocations.
    #[arg(long, default_value_t = false)]
    frozen: bool,

    /// Passed to `cargo` invocations.
    #[arg(long, default_value_t = false)]
    locked: bool,

    /// Passed to `cargo` invocations.
    #[arg(long, default_value_t = false)]
    offline: bool,

    /// Attempt to automatically correct Cargo.toml files. This feature is
    /// currently experimental, and may cause unexpected behavior.
    #[arg(long, default_value_t = false)]
    fix: bool,

    /// Requires nightly, but without this flag the tool make declare
    /// dev-dependencies as unused when they are used.
    #[arg(long, action = clap::ArgAction::Set, default_value_t = true)]
    check_doc_tests: bool,

    /// Whether to allow binaries (such as bins, tests, and examples) to have an
    /// unused dependency on the library artifact.
    #[arg(long, action = clap::ArgAction::Set, default_value_t = false)]
    allow_orphaned_artifacts: bool,

    /// Passed to `cargo` invocations.
    #[arg(long)]
    workspace: bool,

    /// Passed to `cargo` invocations.
    #[arg(long)]
    config: Vec<String>,

    /// Passed to `cargo` invocations.
    #[arg(long)]
    target_dir: Option<PathBuf>,

    /// Passed to `cargo` invocations.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn lint_dependencies(
    workspace: &Path,
    check_doc_tests: bool,
    cargo_args: &CargoArgs,
) -> anyhow::Result<DependencyLintResults> {
    let cargo_version: semver::Version = cargo_version(workspace)?;
    /* TODO: properly match arguments of the cargo check command... */
    let metadata: cargo_metadata::Metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(workspace)
        .features(cargo_metadata::CargoOpt::AllFeatures)
        .exec()?;
    let structured_metadata: StructuredMetadata =
        metadata_to_structured_metadata(&metadata, &cargo_version)?;
    return find_unused_dependencies_all_invocations(
        workspace,
        &CheckTarget::Host,
        &Features::All,
        &structured_metadata,
        check_doc_tests,
        cargo_args,
    );
}

pub fn lib_main(args: &Args) {
    let cargo_version: semver::Version = cargo_version(Path::new(".")).unwrap();
    if !args.workspace {
        assert!(
            supports_default_workspace_members(&cargo_version),
            "You must pass --workspace if cargo is <1.71 due to cargo/cargo_metadata deficiencies"
        );
    }

    let lint_results: DependencyLintResults = lint_dependencies(
        Path::new("."),
        args.check_doc_tests,
        &CargoArgs {
            color: args.color,
            frozen: args.frozen,
            locked: args.locked,
            offline: args.offline,
            workspace: args.workspace,
            config: args.config.clone(),
            target_dir: args.target_dir.clone(),
            manifest_path: args.manifest_path.clone(),
        },
    )
    .unwrap();

    println!("{:#?}", lint_results.unused_dependencies);
    println!(
        "Found #{} unused dependencies",
        lint_results.unused_dependencies.len()
    );

    if !args.allow_orphaned_artifacts {
        println!("{:#?}", lint_results.orphans);
        println!("Found #{} orphan artifacts", lint_results.orphans.len());
    }

    if args.fix {
        for unused_dep in lint_results.unused_dependencies.iter() {
            let manifest_path: &Utf8Path = unused_dep.dependant_manifest_path.as_path();
            let manifest_data: String = std::fs::read_to_string(manifest_path).unwrap();
            /* todo support [target."foo".dependencies] syntax? */
            let mut document = toml_edit::Document::from_str(manifest_data.as_str()).unwrap();

            let mut handled: bool = false;
            for (name, item) in document.iter_mut() {
                if let Some(dep_kind) = toml_key_to_dep_kind(name.get()) {
                    if dep_kind == unused_dep.dep_kind {
                        if let Some(table) = item.as_table_mut() {
                            if table
                                .remove(unused_dep.dependency_name.name.borrow())
                                .is_some()
                            {
                                if handled {
                                    eprintln!("Warning: handled multiple times {:#?}", unused_dep,);
                                }
                                handled = true;
                            }
                        }
                    }
                }
            }
            if !handled {
                eprintln!("Warning: unable to fix {:#?}", unused_dep);
            } else {
                std::fs::write(manifest_path, document.to_string()).unwrap();
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::borrow::Borrow;
    use std::borrow::Cow;

    use cargo_metadata::semver;

    struct UnusedCrateDiagnosticTest {
        message: String,
        crate_name: String,
    }

    #[test]
    fn test_find_crates() {
        let unused_crate_tests: Vec<UnusedCrateDiagnosticTest> = vec![
            UnusedCrateDiagnosticTest {
                message: "external crate `toml` unused in `reves`: remove the dependency or add `use toml as _;`".to_owned(),
                crate_name: "toml".to_owned(),
            },
        ];

        for unused_crate_test in unused_crate_tests.iter() {
            assert_eq!(
                <Cow<'_, str> as Borrow<str>>::borrow(
                    &super::parse_unused_crate_diagnostic(unused_crate_test.message.as_str())
                        .unwrap()
                        .name
                ),
                unused_crate_test.crate_name.as_str()
            );
        }
    }

    #[test]
    fn test_unused_extern_deserialize() {
        let unused_externs: super::UnusedExterns = serde_json::from_str(
            "{\"lint_level\":\"warn\",\"unused_extern_names\":[\"buttercup\",\"lantana\"]}\n",
        )
        .unwrap();
        assert_eq!(
            unused_externs,
            super::UnusedExterns {
                lint_level: "warn".to_owned(),
                unused_extern_names: vec!["buttercup".to_owned(), "lantana".to_owned()],
            }
        );
    }

    struct CargoVersionTest {
        message: &'static str,
        version: semver::Version,
    }

    #[test]
    fn test_cargo_version() {
        let version_tests: &[CargoVersionTest] = &[
            CargoVersionTest {
                message: concat!(
                    "cargo 1.72.1 (103a7ff2e 2023-08-15)\n",
                    "release: 1.72.1\n",
                    "commit-hash: 103a7ff2ee7678d34f34d778614c5eb2525ae9de\n",
                    "commit-date: 2023-08-15\n",
                    "host: x86_64-unknown-linux-gnu\n",
                    "libgit2: 1.6.4 (sys:0.17.2 vendored)\n",
                    "libcurl: 8.1.2-DEV (sys:0.4.63+curl-8.1.2 vendored ssl:OpenSSL/1.1.1u)\n",
                    "ssl: OpenSSL 1.1.1u  30 May 2023\n",
                    "os: Linux 4 (chimaera) [64-bit]\n",
                ),
                version: semver::Version::parse("1.72.1").unwrap(),
            },
            CargoVersionTest {
                message: concat!(
                    "cargo 1.68.0 (115f34552 2023-02-26)\n",
                    "release: 1.68.0\n",
                    "commit-hash: 115f34552518a2f9b96d740192addbac1271e7e6\n",
                    "commit-date: 2023-02-26\n",
                    "host: x86_64-unknown-linux-gnu\n",
                    "libgit2: 1.5.0 (sys:0.16.0 vendored)\n",
                    "libcurl: 7.86.0-DEV (sys:0.4.59+curl-7.86.0 vendored ssl:OpenSSL/1.1.1q)\n",
                    "os: Linux 4 (chimaera) [64-bit]\n",
                ),
                version: semver::Version::parse("1.68.0").unwrap(),
            },
            CargoVersionTest {
                message: concat!(
                    "cargo 1.65.0\n",
                    "release: 1.65.0\n",
                    "host: x86_64-unknown-linux-gnu\n",
                    "libgit2: 1.5.1 (sys:0.16.0 system)\n",
                    "libcurl: 7.88.1 (sys:0.4.59+curl-7.86.0 system ssl:GnuTLS/3.7.9)\n",
                    "os: Linux [64-bit]\n",
                ),
                version: semver::Version::parse("1.65.0").unwrap(),
            },
        ];
        for version_test in version_tests.iter() {
            assert_eq!(
                crate::parse_cargo_version_output(version_test.message).unwrap(),
                version_test.version
            );
        }
    }
}
