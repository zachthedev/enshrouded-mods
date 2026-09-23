//! The release bundle, built by `cargo xtask package`.
//!
//! One archive holds Ember's proxy library at its root and the mod's library
//! under `ember/mods/<mod>/`, which is the layout `docs/install.md` shows. A
//! first install is that one archive, and upgrading the loader alone is the one
//! file from Ember's own release.
//!
//! The loader is built once, in Ember's repository, and attached to Ember's
//! release with a digest file. This command downloads that asset and holds it
//! to that digest rather than building a second copy, so the bytes a mod ships
//! and the bytes Ember published are the same file.
//!
//! No `config.json` reaches the archive. Extracting an upgrade over a running
//! server leaves an admin's settings alone, and each library writes its
//! defaults on first start.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail, ensure};
use owo_colors::{OwoColorize, Stream};
use sha2::{Digest as _, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::runner::{Exit, Runner};

// ///////////////////////////////////////////////
// The layout
// ///////////////////////////////////////////////

/// Ember's proxy library, at the archive root.
///
/// The name is the default proxy slot, and Ember's release attaches the asset
/// under it. A server that already has something in that slot installs the
/// library renamed, which is a file rename after extraction rather than a
/// second archive.
const LOADER: &str = "POWRPROF.dll";

/// The digest file a release carries beside its assets.
///
/// Ember's release writes one for the loader, and this command writes one for
/// the archive it builds. Both are the `sha256sum` format: a hex digest, two
/// spaces, and a file name.
const SUMS: &str = "SHA256SUMS";

/// The directory every mod's library lives in, below the archive root.
const MODS: &str = "ember/mods";

/// The target a shipped library is built for.
///
/// Keen ships a Windows dedicated server only. A Linux host runs it under Wine
/// or Proton, which loads the same Windows libraries.
const TARGET: &str = "x86_64-pc-windows-msvc";

/// The repository Ember's loader is released from.
const EMBER: &str = "zachthedev/enshrouded-ember";

/// The crate whose version names the Ember release a bundle takes its loader
/// from.
const EMBER_SDK: &str = "ember-sdk";

/// The workspace directory that holds the mods.
const MODS_DIRECTORY: &str = "mods";

// ///////////////////////////////////////////////
// Tags
// ///////////////////////////////////////////////

/// A release tag, split into the package it names and the version it carries.
///
/// release-plz tags a workspace package as `<package>-v<version>`, and the
/// archive is named for the whole tag. Splitting it is what lets the command
/// refuse a tag naming another package or another version, either of which
/// would produce an archive named for something it does not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// The package the tag names.
    pub package: String,
    /// The version the tag carries, without its `v`.
    pub version: String,
}

impl Tag {
    /// Read `tag` as `<package>-v<version>`.
    ///
    /// The split is the last `-v` whose remainder is a version, so a package
    /// name that itself ends in `-v` something is read whole.
    ///
    /// # Errors
    ///
    /// Returns an error when the tag carries no such split.
    pub fn parse(tag: &str) -> anyhow::Result<Self> {
        let split = tag
            .match_indices("-v")
            .filter(|(at, _)| is_version(&tag[at + 2..]) && *at > 0)
            .last();
        let Some((at, _)) = split else {
            bail!("{tag} is not a release tag, which is spelled <package>-v<version>");
        };
        Ok(Self {
            package: tag[..at].to_string(),
            version: tag[at + 2..].to_string(),
        })
    }
}

/// Whether `release` is three runs of digits separated by dots.
#[must_use]
pub fn is_release(release: &str) -> bool {
    let parts: Vec<&str> = release.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `version` is three runs of digits, optionally followed by a
/// pre-release the hyphen introduces.
///
/// A shape rather than a parse: the version is compared to the one a manifest
/// declares and never ordered against another.
fn is_version(version: &str) -> bool {
    match version.split_once('-') {
        None => is_release(version),
        Some((release, pre)) => is_release(release) && is_prerelease(pre),
    }
}

/// Whether `pre` is a semver pre-release: dot-separated identifiers, each a
/// non-empty run of ASCII letters, digits and hyphens.
///
/// The whole tag becomes the archive's file name, so the character class is
/// what keeps a version from carrying a path separator, a drive letter or an
/// empty segment into that name.
fn is_prerelease(pre: &str) -> bool {
    !pre.is_empty()
        && pre.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

// ///////////////////////////////////////////////
// The mods this repository can bundle
// ///////////////////////////////////////////////

/// One mod, as its manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mod {
    /// The package name, which names the mod's directory in the install layout.
    pub package: String,
    /// The built library's file name, such as `private_chests.dll`.
    pub library: String,
    /// The version the manifest resolves to.
    pub version: String,
}

impl Mod {
    /// Where this mod's library sits inside the archive.
    #[must_use]
    pub fn entry(&self) -> String {
        format!("{MODS}/{}/{}", self.package, self.library)
    }
}

/// Every mod the workspace declares, in the order the members are listed.
///
/// A member under [`MODS_DIRECTORY`] is a mod. `crates/` holds the libraries
/// mods share and `xtask` is this program, and neither ships.
///
/// # Errors
///
/// Returns an error when a manifest cannot be read or does not declare the
/// package fields a bundle needs.
pub fn mods(root: &Path) -> anyhow::Result<Vec<Mod>> {
    let path = root.join("Cargo.toml");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let workspace: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let inherited = workspace
        .get("workspace")
        .and_then(|table| table.get("package"))
        .and_then(|table| table.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_string);

    let members = workspace
        .get("workspace")
        .and_then(|table| table.get("members"))
        .and_then(toml::Value::as_array)
        .with_context(|| format!("{} lists no workspace members", path.display()))?;

    let mut found = Vec::new();
    for member in members.iter().filter_map(toml::Value::as_str) {
        let Some(directory) = member.strip_prefix(&format!("{MODS_DIRECTORY}/")) else {
            continue;
        };
        let manifest = root.join(MODS_DIRECTORY).join(directory).join("Cargo.toml");
        let text = fs::read_to_string(&manifest)
            .with_context(|| format!("reading {}", manifest.display()))?;
        found.push(
            read_mod(&text, inherited.as_deref())
                .with_context(|| manifest.display().to_string())?,
        );
    }
    Ok(found)
}

/// Read one mod out of its manifest text, with `inherited` standing in for a
/// version the manifest takes from the workspace.
///
/// The library's file name comes from `[lib] name` where the manifest sets one,
/// and from the package name with its hyphens replaced otherwise, which is the
/// name cargo gives the artifact.
///
/// # Errors
///
/// Returns an error when the manifest declares no package name, or no version
/// this function can resolve.
fn read_mod(text: &str, inherited: Option<&str>) -> anyhow::Result<Mod> {
    let manifest: toml::Value = toml::from_str(text).context("parsing the manifest")?;
    let package = manifest
        .get("package")
        .context("the manifest declares no [package]")?;
    let name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .context("the manifest declares no package name")?;

    let declared = package.get("version");
    let version = if let Some(version) = declared.and_then(toml::Value::as_str) {
        version.to_string()
    } else {
        let from_workspace = declared
            .and_then(|value| value.get("workspace"))
            .and_then(toml::Value::as_bool)
            == Some(true);
        ensure!(
            from_workspace,
            "the manifest declares no version for {name}"
        );
        inherited
            .context("the manifest takes its version from a workspace that declares none")?
            .to_string()
    };

    let library = manifest
        .get("lib")
        .and_then(|table| table.get("name"))
        .and_then(toml::Value::as_str)
        .map_or_else(|| name.replace('-', "_"), str::to_string);

    Ok(Mod {
        package: name.to_string(),
        library: format!("{library}.dll"),
        version,
    })
}

// ///////////////////////////////////////////////
// Digests
// ///////////////////////////////////////////////

/// The SHA-256 of `bytes`, as lowercase hex.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            write!(text, "{byte:02x}").expect("a String never fails to be written to");
            text
        })
}

/// One line of a digest file, in the format `sha256sum` reads.
#[must_use]
pub fn sums_line(digest: &str, name: &str) -> String {
    format!("{digest}  {name}\n")
}

/// The digest `sums` records for `name`, or `None` when it records none.
///
/// A line is a digest, whitespace, and a name. The binary marker `*` opens the
/// name in the form `sha256sum -b` writes, and the name is compared without it.
///
/// Every line is read rather than the first match taken, because a file
/// recording one name twice with two digests says nothing about which is right.
/// `sha256sum -c` fails such a file, and so does this.
///
/// # Errors
///
/// Returns an error when `sums` records `name` with more than one digest, or
/// records it with something that is not a SHA-256 digest.
pub fn recorded_digest(sums: &str, name: &str) -> anyhow::Result<Option<String>> {
    let mut found: Vec<String> = sums
        .lines()
        .filter_map(|line| {
            let (digest, rest) = line.split_once(char::is_whitespace)?;
            let recorded = rest.trim_start().trim_start_matches('*').trim_end();
            (recorded == name).then(|| digest.trim().to_lowercase())
        })
        .collect();
    found.dedup();

    match found.len() {
        0 => Ok(None),
        1 => {
            let digest = found.remove(0);
            ensure!(
                is_digest(&digest),
                "{name} is recorded as {digest:?}, which is not a SHA-256 digest"
            );
            Ok(Some(digest))
        }
        _ => bail!("{name} is recorded with more than one digest: {found:?}"),
    }
}

/// Whether `digest` is the 64 lowercase hex characters a SHA-256 is written as.
fn is_digest(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

// ///////////////////////////////////////////////
// The Ember release a bundle takes its loader from
// ///////////////////////////////////////////////

/// The source line cargo writes for a crate that comes from crates.io.
///
/// Cargo writes the index's canonical address whichever protocol fetched it, so
/// one string covers the sparse and the git index alike.
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// The prefix every crate in Ember's workspace is named with.
const EMBER_PREFIX: &str = "ember-";

/// What the lockfile records for one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locked {
    /// The package's name.
    pub name: String,
    /// The version cargo settled on, which is the version the mod is compiled
    /// against.
    pub version: String,
    /// Where that version came from. Cargo writes no source for a crate that
    /// comes from a directory.
    pub source: Option<String>,
}

/// One lockfile package and the dependencies it names.
struct Entry {
    locked: Locked,
    dependencies: Vec<String>,
}

/// Every package the lockfile records.
fn entries(lockfile: &str) -> anyhow::Result<Vec<Entry>> {
    let lock: toml::Value = toml::from_str(lockfile).context("parsing the lockfile")?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .context("the lockfile records no packages")?;
    Ok(packages
        .iter()
        .filter_map(|package| {
            let text = |key: &str| package.get(key).and_then(toml::Value::as_str);
            let locked = Locked {
                name: text("name")?.to_string(),
                version: text("version")?.to_string(),
                source: text("source").map(str::to_string),
            };
            let dependencies = package
                .get("dependencies")
                .and_then(toml::Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(toml::Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            Some(Entry {
                locked,
                dependencies,
            })
        })
        .collect())
}

/// The one entry for [`EMBER_SDK`].
fn sdk_entry(entries: &[Entry]) -> anyhow::Result<&Entry> {
    let mut found: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.locked.name == EMBER_SDK)
        .collect();
    found.dedup_by(|one, other| one.locked == other.locked);

    match found.as_slice() {
        [one] => Ok(one),
        [] => {
            bail!("the lockfile records no {EMBER_SDK}, so nothing says which Ember release fits")
        }
        many => {
            let records: Vec<&Locked> = many.iter().map(|entry| &entry.locked).collect();
            bail!("the lockfile records {EMBER_SDK} as {records:?}, so no one release fits")
        }
    }
}

/// The entry a lockfile dependency line names.
///
/// Cargo writes the name alone where the lockfile holds one package of that
/// name, and adds the version, then the source in parentheses, as far as it
/// takes to tell two apart.
fn resolve<'a>(entries: &'a [Entry], dependency: &str) -> anyhow::Result<&'a Entry> {
    let mut words = dependency.splitn(3, ' ');
    let name = words.next().unwrap_or_default();
    let version = words.next();
    let source = words
        .next()
        .map(|source| source.trim_start_matches('(').trim_end_matches(')'));
    let found: Vec<&Entry> = entries
        .iter()
        .filter(|entry| {
            entry.locked.name == name
                && version.is_none_or(|version| entry.locked.version == version)
                && source.is_none_or(|source| entry.locked.source.as_deref() == Some(source))
        })
        .collect();

    match found.as_slice() {
        [one] => Ok(one),
        [] => bail!("the lockfile names the dependency {dependency:?} and records no such package"),
        _ => bail!(
            "the lockfile names the dependency {dependency:?}, which fits more than one package"
        ),
    }
}

/// The lockfile's record of [`EMBER_SDK`].
///
/// The lockfile rather than the requirement in the manifest: a requirement
/// admits a range, and the release whose loader matches this build is the one
/// version cargo settled on.
///
/// # Errors
///
/// Returns an error when the lockfile records no such package, or records it
/// more than once.
pub fn ember_locked(lockfile: &str) -> anyhow::Result<Locked> {
    Ok(sdk_entry(&entries(lockfile)?)?.locked.clone())
}

/// Every Ember crate the mod compiles: [`EMBER_SDK`] first, then each crate
/// named with [`EMBER_PREFIX`] that it reaches through the lockfile.
///
/// # Errors
///
/// Returns an error when the lockfile does not record the sdk once, or names a
/// dependency it records no single package for.
pub fn ember_family(lockfile: &str) -> anyhow::Result<Vec<Locked>> {
    let entries = entries(lockfile)?;
    let mut reached: Vec<&Entry> = vec![sdk_entry(&entries)?];
    let mut next = 0;
    while let Some(entry) = reached.get(next).copied() {
        next += 1;
        for dependency in &entry.dependencies {
            if !dependency.starts_with(EMBER_PREFIX) {
                continue;
            }
            let found = resolve(&entries, dependency)?;
            if !reached.iter().any(|seen| std::ptr::eq(*seen, found)) {
                reached.push(found);
            }
        }
    }
    Ok(reached
        .into_iter()
        .map(|entry| entry.locked.clone())
        .collect())
}

/// The tag Ember releases `version` under.
///
/// Ember publishes one version for its whole workspace, so one release covers
/// every crate in it and the tag carries no package name.
#[must_use]
pub fn ember_tag(version: &str) -> String {
    format!("v{version}")
}

/// The Ember release tag for the sdk version `lockfile` resolves.
///
/// Only a build whose Ember crates all come from crates.io names a release. A
/// crate from a directory or from git carries whatever its checkout says, so
/// its version matches a release's number without matching its code.
///
/// # Errors
///
/// Returns an error when the lockfile does not resolve one sdk version, and
/// when any Ember crate the sdk reaches does not come from crates.io.
pub fn ember_release(lockfile: &str) -> anyhow::Result<String> {
    let family = ember_family(lockfile)?;
    for locked in &family {
        ensure!(
            locked.source.as_deref() == Some(CRATES_IO),
            "{} {} comes from {}, not crates.io, so no {EMBER} release is known to match it; \
             build against the published crates, or pass --ember-loader with a loader already on \
             disk",
            locked.name,
            locked.version,
            locked.source.as_deref().unwrap_or("a directory")
        );
    }
    Ok(ember_tag(&family[0].version))
}

// ///////////////////////////////////////////////
// Where the loader comes from
// ///////////////////////////////////////////////

/// The loader and the digest file it is held to, both on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// The loader library.
    pub loader: PathBuf,
    /// The digest file that records the loader's digest.
    pub sums: PathBuf,
}

/// Where a bundle's loader comes from.
///
/// The download is the only part of this command a test substitutes. Everything
/// after it, the digest check included, runs over real files either way.
pub trait Source {
    /// Put the loader and its digest file on disk, staging under `into` where
    /// the source has to fetch them.
    ///
    /// # Errors
    ///
    /// Returns an error when the assets cannot be reached or are not both
    /// there.
    fn stage(&self, into: &Path) -> anyhow::Result<Staged>;

    /// One line saying where the loader came from, for the run's own output.
    ///
    /// A reader of a release job's log has the digest check in front of them
    /// either way, and this is what says what that check proves.
    fn origin(&self) -> String;
}

/// Ember's GitHub release, reached through the `gh` command line.
///
/// `gh` rather than an HTTP client in this process: it already carries the
/// credential a release download needs, it is what the release workflow uses to
/// attach the archive this command builds, and it keeps a TLS stack out of a
/// build tool.
pub struct Release<'a> {
    runner: &'a dyn Runner,
    tag: String,
}

impl<'a> Release<'a> {
    /// Build a source that downloads Ember's release `tag` through `runner`.
    #[must_use]
    pub fn new(runner: &'a dyn Runner, tag: String) -> Self {
        Self { runner, tag }
    }

    /// The command that downloads both assets of `tag` into `into`.
    #[must_use]
    pub fn command(tag: &str, into: &Path) -> Vec<String> {
        [
            "gh",
            "release",
            "download",
            tag,
            "--repo",
            EMBER,
            "--pattern",
            LOADER,
            "--pattern",
            SUMS,
            "--dir",
            &into.to_string_lossy(),
        ]
        .iter()
        .map(|word| (*word).to_string())
        .collect()
    }
}

impl Source for Release<'_> {
    fn stage(&self, into: &Path) -> anyhow::Result<Staged> {
        let command = Self::command(&self.tag, into);
        let borrowed: Vec<&str> = command.iter().map(String::as_str).collect();
        let line = command.join(" ");
        match self
            .runner
            .run(&borrowed, &[])
            .with_context(|| format!("failed to start: {line}"))?
        {
            Exit::Ok => {}
            Exit::Err => bail!(
                "{EMBER} has no release {} carrying {LOADER} and {SUMS}, or it cannot be reached: \
                 {line}",
                self.tag
            ),
        }
        Ok(Staged {
            loader: into.join(LOADER),
            sums: into.join(SUMS),
        })
    }

    fn origin(&self) -> String {
        format!("{LOADER} from the {EMBER} release {}", self.tag)
    }
}

/// A loader already on disk, with its digest file beside it.
///
/// This is what `--ember-loader` selects. Both routes run the same digest
/// check, and it proves a different thing on each. The download takes the
/// loader and the digest file from one release, so the digest catches a
/// transfer that went wrong. Here the digest file comes from the loader's own
/// directory, so whoever wrote one wrote the other, and agreement between them
/// says nothing about where either came from. The flag is a development
/// convenience, and [`Source::origin`] is what says so in the run's output.
pub struct Local {
    loader: PathBuf,
}

impl Local {
    /// Build a source over the loader at `loader`.
    #[must_use]
    pub fn new(loader: PathBuf) -> Self {
        Self { loader }
    }
}

impl Source for Local {
    fn stage(&self, _into: &Path) -> anyhow::Result<Staged> {
        // The archive puts whatever this yields in the LOADER slot, so a file
        // under another name would ship as a library it is not.
        // The digest file records an asset under its own name, so an exact
        // match is what lets the check that follows find the right line.
        let named = self.loader.file_name().is_some_and(|name| name == LOADER);
        ensure!(
            named,
            "{} is not named {LOADER}, and the archive would carry it under that name anyway",
            self.loader.display()
        );
        let sums = self
            .loader
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(SUMS);
        Ok(Staged {
            loader: self.loader.clone(),
            sums,
        })
    }

    fn origin(&self) -> String {
        format!(
            "{LOADER} from {}, which its own {SUMS} agrees with rather than a release",
            self.loader.display()
        )
    }
}

/// Read the staged loader and hold it to the digest its file records.
///
/// # Errors
///
/// Returns an error when either file is unreadable, when the digest file
/// records nothing for the loader, or when the digests disagree.
fn verified(staged: &Staged) -> anyhow::Result<Vec<u8>> {
    let bytes = fs::read(&staged.loader)
        .with_context(|| format!("reading the loader at {}", staged.loader.display()))?;
    let sums = fs::read_to_string(&staged.sums)
        .with_context(|| format!("reading the loader's digests at {}", staged.sums.display()))?;
    let name = staged.loader.file_name().map_or_else(
        || LOADER.to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let recorded = recorded_digest(&sums, &name)
        .with_context(|| format!("reading {}", staged.sums.display()))?
        .with_context(|| format!("{} records no digest for {name}", staged.sums.display()))?;
    let found = digest(&bytes);
    ensure!(
        found == recorded,
        "{} hashes to {found} and {} records {recorded}",
        staged.loader.display(),
        staged.sums.display()
    );
    Ok(bytes)
}

// ///////////////////////////////////////////////
// Building the mod
// ///////////////////////////////////////////////

/// The command that builds `package` for the shipped target.
///
/// The target directory is named rather than left to the environment, so the
/// artifact this command reads and the artifact cargo wrote are the same file
/// whatever `CARGO_TARGET_DIR` holds.
#[must_use]
pub fn build_command(package: &str, target_directory: &Path) -> Vec<String> {
    [
        "cargo",
        "build",
        "--release",
        "--locked",
        "--lib",
        "--package",
        package,
        "--target",
        TARGET,
        "--target-dir",
        &target_directory.to_string_lossy(),
    ]
    .iter()
    .map(|word| (*word).to_string())
    .collect()
}

/// Where cargo writes the shipped library, given the target directory.
#[must_use]
pub fn artifact(target_directory: &Path, library: &str) -> PathBuf {
    target_directory.join(TARGET).join("release").join(library)
}

/// Build `subject` and report the library cargo wrote.
///
/// # Errors
///
/// Returns an error when cargo cannot be started, when it exits non-zero, or
/// when the artifact is not where the build was told to put it.
fn build(
    runner: &dyn Runner,
    subject: &Mod,
    target_directory: &Path,
    out: &mut dyn Write,
) -> anyhow::Result<PathBuf> {
    let command = build_command(&subject.package, target_directory);
    let borrowed: Vec<&str> = command.iter().map(String::as_str).collect();
    let line = command.join(" ");
    writeln!(
        out,
        "{}",
        line.if_supports_color(Stream::Stdout, OwoColorize::dimmed)
    )
    .context("failed to write to the terminal")?;

    match runner
        .run(&borrowed, &[])
        .with_context(|| format!("failed to start: {line}"))?
    {
        Exit::Ok => {}
        Exit::Err => bail!("building {} failed", subject.package),
    }

    let built = artifact(target_directory, &subject.library);
    ensure!(
        built.is_file(),
        "the build left no {} at {}",
        subject.library,
        built.display()
    );
    Ok(built)
}

// ///////////////////////////////////////////////
// The archive
// ///////////////////////////////////////////////

/// What one run wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The archive.
    pub archive: PathBuf,
    /// The digest file naming the archive.
    pub sums: PathBuf,
    /// The archive's digest.
    pub digest: String,
}

/// Refuse an output directory that already holds anything.
///
/// A directory that is not there yet holds nothing, and nothing is created
/// here, so this runs before the build without leaving a directory behind on a
/// refusal.
///
/// # Errors
///
/// Returns an error when `out` holds any entry, and when it cannot be read for
/// a reason other than not being there.
pub fn refuse_occupied(out: &Path) -> anyhow::Result<()> {
    let entries = match fs::read_dir(out) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).with_context(|| format!("reading {}", out.display())),
    };
    let existing: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    ensure!(
        existing.is_empty(),
        "{} already holds {existing:?}, and a bundle is written into an empty directory",
        out.display()
    );
    Ok(())
}

/// Refuse an archive entry that could be extracted outside the archive's root.
///
/// A zip entry is a string, and an extractor joins it onto the directory it is
/// pointed at. An entry climbing out of that directory writes wherever it says,
/// which for this archive is a game server's own directory.
///
/// The entries here are built from a manifest rather than from a stranger's
/// input, so this guards a constructed value. It is one pass over a short
/// string, and it is the only thing in this file that states the containment
/// the archive relies on.
///
/// # Errors
///
/// Returns an error when `entry` is empty, carries a backslash, a drive letter,
/// an empty component or a `..` component, or opens with a separator.
pub fn refuse_escaping(entry: &str) -> anyhow::Result<()> {
    ensure!(!entry.is_empty(), "an archive entry is empty");
    ensure!(
        !entry.contains('\\'),
        "the archive entry {entry} carries a backslash, which an extractor reads as a separator"
    );
    ensure!(
        !entry.contains(':'),
        "the archive entry {entry} carries a drive letter or a stream name"
    );
    for part in entry.split('/') {
        ensure!(
            !part.is_empty(),
            "the archive entry {entry} carries an empty path component"
        );
        ensure!(
            part != "..",
            "the archive entry {entry} climbs out of the archive root"
        );
    }
    Ok(())
}

/// Write the archive and its digest file into `out`, which has to be empty.
///
/// The archive carries the loader at its root and the mod's library below it,
/// and nothing else. Every entry is stamped with the same fixed time, so two
/// runs over the same inputs write the same bytes and the digest is a fact
/// about the inputs rather than about when the build ran.
///
/// `out` is resolved by the operating system, so a link writes where it points
/// and the emptiness check measures what it points at.
///
/// # Errors
///
/// Returns an error when `out` holds anything already, when either input cannot
/// be read, when an entry could be extracted outside the archive root, or when
/// the archive cannot be written.
pub fn bundle(
    subject: &Mod,
    tag: &str,
    library: &Path,
    loader: &[u8],
    out: &Path,
) -> anyhow::Result<Bundle> {
    let library_entry = subject.entry();
    refuse_escaping(LOADER)?;
    refuse_escaping(&library_entry)?;

    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    refuse_occupied(out)?;

    let compiled = fs::read(library).with_context(|| format!("reading {}", library.display()))?;

    let archive = out.join(format!("{tag}.zip"));
    // create_new rather than create: the emptiness check above says nothing
    // stands here, so a file that does now is one another writer put there, and
    // truncating it silently is the one outcome with no way back.
    let file =
        File::create_new(&archive).with_context(|| format!("creating {}", archive.display()))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());

    for (entry, bytes) in [(LOADER.to_string(), loader), (library_entry, &compiled[..])] {
        writer
            .start_file(entry.clone(), options)
            .with_context(|| format!("starting {entry} in {}", archive.display()))?;
        writer
            .write_all(bytes)
            .with_context(|| format!("writing {entry} into {}", archive.display()))?;
    }
    writer
        .finish()
        .with_context(|| format!("closing {}", archive.display()))?;

    let written =
        fs::read(&archive).with_context(|| format!("reading back {}", archive.display()))?;
    let digest = digest(&written);
    let name = archive
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let sums = out.join(SUMS);
    let mut digests =
        File::create_new(&sums).with_context(|| format!("creating {}", sums.display()))?;
    digests
        .write_all(sums_line(&digest, &name).as_bytes())
        .with_context(|| format!("writing {}", sums.display()))?;

    Ok(Bundle {
        archive,
        sums,
        digest,
    })
}

// ///////////////////////////////////////////////
// The command
// ///////////////////////////////////////////////

/// What `cargo xtask package` was asked for.
#[derive(Debug, Clone)]
pub struct Request {
    /// The mod to bundle, by its package name.
    pub subject: String,
    /// The release tag the archive is named for.
    pub tag: String,
    /// The directory the archive and its digest file are written into.
    pub out: PathBuf,
    /// A loader already on disk, with its digest file beside it, in place of
    /// the download.
    pub loader: Option<PathBuf>,
}

/// Build the bundle `request` describes.
///
/// Every refusal that reads only the request and this repository's own files
/// runs first: the tag, the mod, the output directory, and the Ember release
/// the loader comes from. The loader is staged and checked next, because a
/// digest that disagrees costs seconds to find and the build costs minutes.
/// The build is last of the steps that can fail before anything is written.
///
/// # Errors
///
/// Returns an error when the request names a mod this workspace does not hold,
/// when the tag disagrees with the mod's manifest, when the output directory
/// holds anything, when the loader fails its digest check, or when any step of
/// the build or the write fails.
pub fn run(
    runner: &dyn Runner,
    root: &Path,
    request: &Request,
    out: &mut dyn Write,
) -> anyhow::Result<Bundle> {
    let tag = Tag::parse(&request.tag)?;
    let known = mods(root)?;
    let subject = known
        .iter()
        .find(|candidate| candidate.package == request.subject)
        .with_context(|| {
            let names: Vec<&str> = known.iter().map(|one| one.package.as_str()).collect();
            format!(
                "this workspace holds no mod named {}, it holds {names:?}",
                request.subject
            )
        })?;
    ensure!(
        tag.package == subject.package,
        "{} names {} and the bundle is for {}",
        request.tag,
        tag.package,
        subject.package
    );
    ensure!(
        tag.version == subject.version,
        "{} carries {} and {} is at {}",
        request.tag,
        tag.version,
        subject.package,
        subject.version
    );

    refuse_occupied(&request.out)?;

    let source: Box<dyn Source> = if let Some(path) = &request.loader {
        Box::new(Local::new(path.clone()))
    } else {
        let lockfile = root.join("Cargo.lock");
        let text = fs::read_to_string(&lockfile)
            .with_context(|| format!("reading {}", lockfile.display()))?;
        Box::new(Release::new(runner, ember_release(&text)?))
    };

    let staging = tempfile::TempDir::with_prefix("xtask-package-")
        .context("creating a directory to stage the loader in")?;
    let staged = source.stage(staging.path())?;
    let loader = verified(&staged)?;

    let library = build(runner, subject, &root.join("target"), out)?;
    let bundle = bundle(subject, &request.tag, &library, &loader, &request.out)?;

    writeln!(out, "\n  {}", source.origin())?;
    writeln!(out, "  {}", subject.entry())?;
    writeln!(out, "  {}", bundle.archive.display())?;
    writeln!(out, "  {}", bundle.sums.display())?;
    Ok(bundle)
}

// ///////////////////////////////////////////////
// Tests
// ///////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::{
        Bundle, EMBER, EMBER_SDK, LOADER, Local, Locked, MODS, MODS_DIRECTORY, Mod, Release,
        Request, SUMS, Source, TARGET, Tag, artifact, build_command, bundle, digest, ember_family,
        ember_locked, ember_release, ember_tag, is_release, mods, read_mod, recorded_digest,
        refuse_escaping, run, sums_line, verified,
    };
    use crate::runner::{Exit, Runner};

    /// The bytes a fixture loader holds, which are not a real library and never
    /// reach one.
    const LOADER_BYTES: &[u8] = b"a stand-in for Ember's proxy library";

    /// The bytes a fixture mod library holds.
    const LIBRARY_BYTES: &[u8] = b"a stand-in for a built mod";

    /// The mod every case bundles, which is the one this workspace holds.
    fn subject() -> Mod {
        Mod {
            package: "private-chests".to_string(),
            library: "private_chests.dll".to_string(),
            version: "0.1.0".to_string(),
        }
    }

    /// A sandbox directory, removed when the test ends.
    fn sandbox(label: &str) -> TempDir {
        TempDir::with_prefix(format!("xtask-package-test-{label}-")).expect("a temporary directory")
    }

    /// Write `bytes` at `path`, creating the directories above it.
    fn put(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the parent directory");
        }
        fs::write(path, bytes).expect("the file");
    }

    /// Lay a loader and its digest file into `directory`, with `recorded`
    /// standing in for the digest when the case wants them to disagree.
    fn stage_loader(directory: &Path, bytes: &[u8], recorded: Option<&str>) -> PathBuf {
        let loader = directory.join(LOADER);
        put(&loader, bytes);
        let recorded = recorded.map_or_else(|| digest(bytes), str::to_string);
        put(
            &directory.join(SUMS),
            sums_line(&recorded, LOADER).as_bytes(),
        );
        loader
    }

    /// A workspace root under `home`, carrying the manifests and the lockfile
    /// this command reads, copied from the real one.
    ///
    /// Copied rather than written by hand: the case then reads the same member
    /// list and the same resolved Ember version the repository ships, and a
    /// build writes its artifact inside the sandbox rather than into the
    /// checkout's own target directory.
    fn workspace(home: &Path) -> PathBuf {
        let real = crate::repo_root();
        let root = home.join("workspace");
        for relative in [
            Path::new("Cargo.toml"),
            Path::new("Cargo.lock"),
            &Path::new(MODS_DIRECTORY)
                .join("private-chests")
                .join("Cargo.toml"),
        ] {
            let copy = root.join(relative);
            fs::create_dir_all(copy.parent().expect("a parent")).expect("the directory");
            fs::copy(real.join(relative), &copy)
                .unwrap_or_else(|err| panic!("{}: {err}", relative.display()));
        }
        root
    }

    /// Every entry of the archive at `path`, in the order it holds them.
    fn entries(path: &Path) -> Vec<String> {
        let file = fs::File::open(path).expect("the archive opens");
        let mut archive = zip::ZipArchive::new(file).expect("the archive reads as a zip");
        (0..archive.len())
            .map(|index| {
                archive
                    .by_index(index)
                    .expect("an entry")
                    .name()
                    .to_string()
            })
            .collect()
    }

    /// The bytes the archive at `path` holds under `entry`.
    fn entry_bytes(path: &Path, entry: &str) -> Vec<u8> {
        use std::io::Read as _;
        let file = fs::File::open(path).expect("the archive opens");
        let mut archive = zip::ZipArchive::new(file).expect("the archive reads as a zip");
        let mut found = Vec::new();
        archive
            .by_name(entry)
            .unwrap_or_else(|err| panic!("{entry}: {err}"))
            .read_to_end(&mut found)
            .expect("the entry reads");
        found
    }

    /// Build a bundle in `out` from fixture inputs.
    fn fixture_bundle(out: &Path, library: &Path) -> Bundle {
        bundle(
            &subject(),
            "private-chests-v0.1.0",
            library,
            LOADER_BYTES,
            out,
        )
        .expect("the bundle is written")
    }

    // ///// The archive /////

    /// The archive holds the loader at its root and the mod's library below it,
    /// and nothing else. A `config.json` in it would replace an admin's
    /// settings when an upgrade is extracted over a running server.
    #[test]
    fn the_archive_holds_the_loader_and_the_library_and_no_settings_file() {
        let home = sandbox("entries");
        let library = home.path().join("private_chests.dll");
        put(&library, LIBRARY_BYTES);
        let out = home.path().join("out");

        let written = fixture_bundle(&out, &library);
        let held = entries(&written.archive);

        assert_eq!(
            held,
            vec![
                LOADER.to_string(),
                format!("{MODS}/private-chests/private_chests.dll"),
            ]
        );
        assert!(
            !held.iter().any(|entry| entry.ends_with("config.json")),
            "the archive ships settings: {held:?}"
        );
        assert_eq!(entry_bytes(&written.archive, LOADER), LOADER_BYTES);
        assert_eq!(
            entry_bytes(&written.archive, &subject().entry()),
            LIBRARY_BYTES
        );
    }

    /// The output directory holds the archive and its digest file and nothing
    /// else, and the digest file records the archive's own bytes under the name
    /// the tag gives it.
    #[test]
    fn the_output_directory_holds_the_archive_and_its_digest_and_nothing_else() {
        let home = sandbox("digest");
        let library = home.path().join("private_chests.dll");
        put(&library, LIBRARY_BYTES);
        let out = home.path().join("out");

        let written = fixture_bundle(&out, &library);

        let mut held: Vec<String> = fs::read_dir(&out)
            .expect("the output directory reads")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        held.sort();
        assert_eq!(
            held,
            vec![SUMS.to_string(), "private-chests-v0.1.0.zip".to_string()]
        );

        let sums = fs::read_to_string(&written.sums).expect("the digest file reads");
        let recorded = recorded_digest(&sums, "private-chests-v0.1.0.zip")
            .expect("the digest file reads")
            .expect("the digest file names the archive");
        let found = digest(&fs::read(&written.archive).expect("the archive reads"));
        assert_eq!(
            recorded, found,
            "the digest file does not match the archive"
        );
        assert_eq!(recorded, written.digest);
    }

    /// Two runs over the same inputs write the same archive. A digest that
    /// moved with the clock would say nothing about what the archive holds, and
    /// a release rebuilt from the same tag would not reproduce.
    #[test]
    fn two_runs_over_the_same_inputs_write_the_same_archive() {
        let home = sandbox("reproducible");
        let library = home.path().join("private_chests.dll");
        put(&library, LIBRARY_BYTES);

        let first = fixture_bundle(&home.path().join("first"), &library);
        let second = fixture_bundle(&home.path().join("second"), &library);

        assert_eq!(first.digest, second.digest);
        assert_eq!(
            fs::read(&first.archive).expect("the first archive reads"),
            fs::read(&second.archive).expect("the second archive reads")
        );
    }

    /// An output directory holding anything is refused rather than added to. A
    /// release job uploads what it finds there, so a previous run left in place
    /// would be published beside this one.
    #[test]
    fn a_bundle_refuses_an_output_directory_that_holds_anything() {
        let home = sandbox("occupied");
        let library = home.path().join("private_chests.dll");
        put(&library, LIBRARY_BYTES);
        let out = home.path().join("out");
        put(&out.join("private-chests-v0.0.9.zip"), b"an earlier run");

        let err = bundle(
            &subject(),
            "private-chests-v0.1.0",
            &library,
            LOADER_BYTES,
            &out,
        )
        .expect_err("a non-empty output directory is refused");

        let said = format!("{err:#}");
        assert!(said.contains("private-chests-v0.0.9.zip"), "got {said}");
        assert!(
            !out.join("private-chests-v0.1.0.zip").exists(),
            "it wrote the archive anyway"
        );
    }

    /// An archive entry that could be extracted outside the archive root is
    /// refused, whatever the manifest that produced it said.
    ///
    /// Driven through `bundle` rather than through an assertion over
    /// `Mod::entry`: an assertion whose expected prefix is built from the same
    /// field the entry is built from holds for every value, this one included,
    /// so it cannot see the defect it looks like it covers.
    #[test]
    fn an_entry_that_escapes_the_archive_root_is_refused() {
        let cases = [
            (
                "a parent climb in the package",
                "../../../../pwned",
                "private_chests.dll",
            ),
            (
                "a parent climb in the library",
                "private-chests",
                "../../pwned.dll",
            ),
            ("an absolute package", "/absolute", "private_chests.dll"),
            (
                "a drive letter",
                "C:/Windows/System32",
                "private_chests.dll",
            ),
            ("an empty package", "", "private_chests.dll"),
            ("a backslash", r"..\..\pwned", "private_chests.dll"),
            ("a bare parent", "..", "private_chests.dll"),
        ];

        for (label, package, library) in cases {
            let home = sandbox("escape");
            let built = home.path().join("private_chests.dll");
            put(&built, LIBRARY_BYTES);
            let out = home.path().join("out");
            let subject = Mod {
                package: package.to_string(),
                library: library.to_string(),
                version: "0.1.0".to_string(),
            };

            let err = bundle(
                &subject,
                "private-chests-v0.1.0",
                &built,
                LOADER_BYTES,
                &out,
            )
            .expect_err(label);

            let said = format!("{err:#}");
            assert!(
                said.contains("archive entry"),
                "{label}: the refusal is not about the entry: {said}"
            );
            assert!(
                !out.join("private-chests-v0.1.0.zip").exists(),
                "{label}: it wrote the archive anyway"
            );
        }
    }

    /// The entries this command actually writes pass the guard, so the refusal
    /// above is not simply refusing everything.
    #[test]
    fn the_entries_a_bundle_writes_pass_the_containment_guard() {
        let allowed = [
            LOADER,
            "ember/mods/private-chests/private_chests.dll",
            "ember/mods/a-mod/a_mod.dll",
        ];
        for entry in allowed {
            refuse_escaping(entry).unwrap_or_else(|err| panic!("{entry}: {err:#}"));
        }
        assert!(refuse_escaping("").is_err(), "an empty entry is accepted");
    }

    // ///// Tags /////

    /// A tag splits into the package it names and the version it carries, and a
    /// tag that carries no such split is refused rather than guessed at.
    #[test]
    fn a_tag_reads_as_a_package_and_a_version() {
        let cases: Vec<(&str, Option<(&str, &str)>)> = vec![
            ("private-chests-v0.1.0", Some(("private-chests", "0.1.0"))),
            ("mod-v1.2.3", Some(("mod", "1.2.3"))),
            // The last split wins, so a package name carrying its own `-v` is
            // read whole.
            ("my-v2-mod-v0.4.1", Some(("my-v2-mod", "0.4.1"))),
            (
                "private-chests-v1.0.0-rc.1",
                Some(("private-chests", "1.0.0-rc.1")),
            ),
            ("v0.1.0", None),
            ("private-chests", None),
            ("private-chests-v", None),
            ("private-chests-v1.0", None),
            ("private-chests-0.1.0", None),
            ("-v0.1.0", None),
            ("", None),
            // A pre-release is semver's own character class. The tag becomes
            // the archive's file name, so anything that reads as a path there
            // is refused here.
            (
                "private-chests-v0.1.0-rc-1",
                Some(("private-chests", "0.1.0-rc-1")),
            ),
            (
                "private-chests-v0.1.0-alpha.2.3",
                Some(("private-chests", "0.1.0-alpha.2.3")),
            ),
            ("private-chests-v0.1.0-../../../../escape", None),
            ("private-chests-v0.1.0-/etc/passwd", None),
            (r"private-chests-v0.1.0-C:\Windows\evil", None),
            ("private-chests-v0.1.0-x/../../escaped", None),
            ("private-chests-v0.1.0-", None),
            ("private-chests-v0.1.0-rc..1", None),
            ("private-chests-v0.1.0-rc 1", None),
            ("private-chests-v0.1.0+build.7", None),
        ];

        for (tag, expected) in cases {
            let parsed = Tag::parse(tag);
            if let Some((package, version)) = expected {
                let found = parsed.unwrap_or_else(|err| panic!("{tag}: {err}"));
                assert_eq!(found.package, package, "{tag}");
                assert_eq!(found.version, version, "{tag}");
            } else {
                let err = parsed.err().unwrap_or_else(|| panic!("{tag} parsed"));
                assert!(
                    format!("{err:#}").contains("<package>-v<version>"),
                    "{tag}: the refusal does not say what a tag looks like"
                );
            }
        }
    }

    // ///// Digest files /////

    /// A digest file is read in the shapes `sha256sum` writes, and a name it
    /// does not record reads as no digest rather than as another line's.
    #[test]
    fn a_digest_file_is_read_in_the_shapes_sha256sum_writes() {
        let first = "a".repeat(64);
        let second = "B".repeat(64);
        let third = "c".repeat(64);
        let sums = format!("{first}  POWRPROF.dll\n{second} *ember.dll\n{third}   spaced.dll\n");
        let cases: Vec<(&str, Option<String>)> = vec![
            ("POWRPROF.dll", Some(first.clone())),
            // The binary marker opens the name and is not part of it, and a
            // recorded digest is compared without case.
            ("ember.dll", Some(second.to_lowercase())),
            ("spaced.dll", Some(third.clone())),
            ("absent.dll", None),
            ("POWRPROF", None),
            ("", None),
        ];

        for (name, expected) in cases {
            assert_eq!(
                recorded_digest(&sums, name).expect("the digest file reads"),
                expected,
                "{name}"
            );
        }
        assert_eq!(
            recorded_digest("", "POWRPROF.dll").expect("an empty digest file reads"),
            None
        );
    }

    /// A digest file recording one name twice with two digests says nothing
    /// about which is right, so it is refused rather than resolved by order.
    /// The same digest written twice is not a disagreement.
    #[test]
    fn a_digest_file_naming_one_asset_twice_is_refused() {
        let first = "a".repeat(64);
        let other = "c".repeat(64);

        let disagreeing = format!("{first}  {LOADER}\n{other}  {LOADER}\n");
        let err = recorded_digest(&disagreeing, LOADER).expect_err("two digests are refused");
        assert!(
            format!("{err:#}").contains("more than one digest"),
            "got {err:#}"
        );

        let repeated = format!("{first}  {LOADER}\n{first}  {LOADER}\n");
        assert_eq!(
            recorded_digest(&repeated, LOADER).expect("one digest written twice reads"),
            Some(first)
        );
    }

    /// A recorded value that is not 64 hex characters is refused where it is
    /// read, rather than failing later as a comparison that happens not to
    /// match.
    #[test]
    fn a_recorded_value_that_is_not_a_digest_is_refused() {
        let full = "a".repeat(64);
        let cases = [
            ("an empty digest field", format!("  {LOADER}")),
            ("a short digest", format!("aa11  {LOADER}")),
            ("a long digest", format!("{full}a  {LOADER}")),
            (
                "a digest carrying a non-hex character",
                format!("{}z  {LOADER}", "a".repeat(63)),
            ),
        ];

        for (label, sums) in cases {
            let err = recorded_digest(&sums, LOADER).expect_err(label);
            assert!(
                format!("{err:#}").contains("not a SHA-256 digest"),
                "{label}: got {err:#}"
            );
        }
    }

    /// The digest is SHA-256, held to the published vector for the empty input
    /// rather than to whatever this code produces today.
    #[test]
    fn the_digest_is_sha256() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sums_line("abcd", "one.zip"), "abcd  one.zip\n");
    }

    // ///// Manifests /////

    /// A mod's library name, its package name and its version come from its
    /// manifest, with a workspace version resolved rather than reported as
    /// absent.
    #[test]
    fn a_manifest_gives_the_package_the_library_and_the_version() {
        /// A manifest to read, as the case's name, the text, the version a
        /// workspace offers, and the mod that has to come out of it.
        type Case = (
            &'static str,
            &'static str,
            Option<&'static str>,
            Option<(&'static str, &'static str, &'static str)>,
        );

        let cases: Vec<Case> = vec![
            (
                "an inherited version",
                "[package]\nname = \"private-chests\"\nversion.workspace = true\n",
                Some("0.1.0"),
                Some(("private-chests", "private_chests.dll", "0.1.0")),
            ),
            (
                "a declared version",
                "[package]\nname = \"private-chests\"\nversion = \"2.3.4\"\n",
                None,
                Some(("private-chests", "private_chests.dll", "2.3.4")),
            ),
            (
                "a named library",
                "[package]\nname = \"private-chests\"\nversion = \"1.0.0\"\n\n[lib]\nname = \
                 \"chests\"\n",
                None,
                Some(("private-chests", "chests.dll", "1.0.0")),
            ),
            (
                "no package table",
                "[lib]\nname = \"chests\"\n",
                Some("0.1.0"),
                None,
            ),
            (
                "no name",
                "[package]\nversion = \"1.0.0\"\n",
                Some("0.1.0"),
                None,
            ),
            (
                "an inherited version with no workspace to inherit from",
                "[package]\nname = \"private-chests\"\nversion.workspace = true\n",
                None,
                None,
            ),
            (
                "no version at all",
                "[package]\nname = \"private-chests\"\n",
                Some("0.1.0"),
                None,
            ),
        ];

        for (label, text, inherited, expected) in cases {
            let found = read_mod(text, inherited);
            match expected {
                Some((package, library, version)) => {
                    let one = found.unwrap_or_else(|err| panic!("{label}: {err:#}"));
                    assert_eq!(
                        one,
                        Mod {
                            package: package.to_string(),
                            library: library.to_string(),
                            version: version.to_string(),
                        },
                        "{label}"
                    );
                }
                None => assert!(found.is_err(), "{label}: it read a mod anyway"),
            }
        }
    }

    /// This workspace's own mods are the members under `mods/`, and nothing
    /// under `crates/` or the xtask itself.
    #[test]
    fn the_workspace_mods_are_the_members_under_the_mods_directory() {
        let found = mods(&crate::repo_root()).expect("the workspace manifests read");

        assert!(!found.is_empty(), "the workspace holds no mod");
        let names: Vec<&str> = found.iter().map(|one| one.package.as_str()).collect();
        assert!(names.contains(&"private-chests"), "got {names:?}");
        assert!(!names.contains(&"xtask"), "the xtask is not a mod");
        assert!(
            !names.contains(&"mods-common"),
            "a shared crate is not a mod"
        );
        for one in &found {
            assert_eq!(
                Path::new(&one.library)
                    .extension()
                    .and_then(|end| end.to_str()),
                Some("dll"),
                "{} builds no library: {}",
                one.package,
                one.library
            );
            // The prefix is a constant. Building it from `one.package` would
            // make the assertion true for every value that field can hold,
            // including one that climbs out of the archive.
            let entry = one.entry();
            assert!(
                entry.starts_with("ember/mods/"),
                "{} lands outside the mods directory: {entry}",
                one.package
            );
            assert!(
                refuse_escaping(&entry).is_ok(),
                "{} produces an entry that can be extracted elsewhere: {entry}",
                one.package
            );
        }
    }

    // ///// The Ember release /////

    /// The Ember release a bundle takes its loader from is named by the version
    /// the lockfile resolved, so the loader and the compiled mod come from one
    /// release.
    #[test]
    fn the_ember_release_is_named_by_the_resolved_sdk_version() {
        let lockfile = "\
[[package]]
name = \"anyhow\"
version = \"1.0.104\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"ember-sdk\"
version = \"0.4.2\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"
";
        assert_eq!(
            ember_locked(lockfile).expect("the lockfile records the sdk"),
            Locked {
                name: "ember-sdk".to_string(),
                version: "0.4.2".to_string(),
                source: Some("registry+https://github.com/rust-lang/crates.io-index".to_string()),
            }
        );
        assert_eq!(ember_tag("0.4.2"), "v0.4.2");

        let absent = "[[package]]\nname = \"anyhow\"\nversion = \"1.0.104\"\n";
        assert!(
            ember_locked(absent).is_err(),
            "a lockfile without the sdk named a release anyway"
        );
        assert!(
            ember_locked("[[package]]\nname = \"anyhow\"\n").is_err(),
            "a lockfile with no version named a release anyway"
        );
        assert!(
            ember_locked("this is not toml = = =").is_err(),
            "an unparseable lockfile named a release anyway"
        );
    }

    /// A lockfile of `(name, version, source, dependencies)` packages, in the
    /// shape cargo writes one.
    fn lockfile(packages: &[(&str, &str, Option<&str>, &[&str])]) -> String {
        use std::fmt::Write as _;

        let mut text = String::new();
        for (name, version, source, dependencies) in packages {
            writeln!(
                text,
                "[[package]]\nname = \"{name}\"\nversion = \"{version}\""
            )
            .expect("writing to a string");
            if let Some(source) = source {
                writeln!(text, "source = \"{source}\"").expect("writing to a string");
            }
            if !dependencies.is_empty() {
                text.push_str("dependencies = [\n");
                for dependency in *dependencies {
                    writeln!(text, " \"{dependency}\",").expect("writing to a string");
                }
                text.push_str("]\n");
            }
            text.push('\n');
        }
        text
    }

    /// Only a build whose Ember crates all come from crates.io names a
    /// release. A directory or a git checkout carries a version that can match
    /// a release's number without matching its code, so each is refused
    /// wherever it sits among the crates the sdk reaches, and the refusal names
    /// the crate, its version, where it came from, and what to do instead.
    ///
    /// The crates.io source is spelled out here rather than taken from the
    /// constant, so each case pins which source is accepted instead of agreeing
    /// with whatever the constant says today.
    #[test]
    fn the_release_lookup_takes_only_ember_crates_from_crates_io() {
        let crates_io = Some("registry+https://github.com/rust-lang/crates.io-index");
        let git = "git+https://github.com/zachthedev/enshrouded-ember?rev=a5bdfb7#a5bdfb7";
        let other = "sparse+https://registry.example/index/";

        let published = lockfile(&[
            ("anyhow", "1.0.104", crates_io, &[]),
            ("ember-enshrouded", "0.4.2", crates_io, &["ember-platform"]),
            ("ember-platform", "0.4.2", crates_io, &[]),
            (
                "ember-sdk",
                "0.4.2",
                crates_io,
                &["anyhow", "ember-enshrouded"],
            ),
        ]);
        assert_eq!(
            ember_release(&published).expect("Ember crates from crates.io name a release"),
            "v0.4.2"
        );

        // The case, its lockfile, the crate and version the refusal names, and
        // the source it names.
        let refused: [(&str, String, &str, &str); 5] = [
            (
                "the sdk from a directory",
                lockfile(&[("ember-sdk", "0.1.0", None, &[])]),
                "ember-sdk 0.1.0",
                "a directory",
            ),
            (
                "the sdk from git",
                lockfile(&[("ember-sdk", "0.1.0", Some(git), &[])]),
                "ember-sdk 0.1.0",
                git,
            ),
            (
                "the sdk from another registry",
                lockfile(&[("ember-sdk", "0.1.0", Some(other), &[])]),
                "ember-sdk 0.1.0",
                other,
            ),
            (
                "a sibling from a directory",
                lockfile(&[
                    ("ember-platform", "0.1.1", None, &[]),
                    ("ember-sdk", "0.1.0", crates_io, &["ember-platform"]),
                ]),
                "ember-platform 0.1.1",
                "a directory",
            ),
            (
                "a sibling reached through another",
                lockfile(&[
                    ("ember-enshrouded", "0.1.0", crates_io, &["ember-platform"]),
                    ("ember-platform", "0.1.1", Some(git), &[]),
                    ("ember-sdk", "0.1.0", crates_io, &["ember-enshrouded"]),
                ]),
                "ember-platform 0.1.1",
                git,
            ),
        ];
        for (label, text, named, source) in refused {
            let err = ember_release(&text).expect_err(&format!("{label} named a release"));
            let said = format!("{err:#}");
            assert!(
                said.contains(named),
                "{label}: the crate and its version are missing: {said}"
            );
            assert!(
                said.contains(source),
                "{label}: the source is missing: {said}"
            );
            assert!(
                said.contains("--ember-loader"),
                "{label}: the refusal does not say what to do instead: {said}"
            );
        }
    }

    /// Where the lockfile holds two packages of one name, cargo names each by
    /// version, and the walk follows the one the sdk names rather than the
    /// first it meets. A name the lockfile cannot settle is refused.
    #[test]
    fn the_walk_follows_the_package_the_dependency_line_names() {
        let crates_io = Some("registry+https://github.com/rust-lang/crates.io-index");
        let family = |named: &str| {
            lockfile(&[
                ("ember-platform", "0.3.0", None, &[]),
                ("ember-platform", "0.4.2", crates_io, &[]),
                ("ember-sdk", "0.4.2", crates_io, &[named]),
            ])
        };

        assert_eq!(
            ember_release(&family("ember-platform 0.4.2"))
                .expect("the published platform is the one named"),
            "v0.4.2"
        );
        let said = format!(
            "{:#}",
            ember_release(&family("ember-platform 0.3.0"))
                .expect_err("the platform from a directory is the one named")
        );
        assert!(said.contains("ember-platform 0.3.0"), "got {said}");

        let said = format!(
            "{:#}",
            ember_family(&family("ember-platform"))
                .expect_err("a bare name over two packages is refused")
        );
        assert!(said.contains("more than one package"), "got {said}");

        let missing = lockfile(&[("ember-sdk", "0.4.2", crates_io, &["ember-platform"])]);
        let said = format!(
            "{:#}",
            ember_family(&missing).expect_err("a named crate with no package is refused")
        );
        assert!(said.contains("no such package"), "got {said}");
    }

    /// This repository's own lockfile names one Ember release, so the command
    /// resolves against the tree it ships from rather than only against
    /// fixtures.
    ///
    /// The version is scanned out of the lockfile here as well, so the case
    /// reads the file by a second path rather than agreeing with the reader it
    /// is about.
    #[test]
    fn this_workspace_resolves_one_ember_release() {
        let lockfile =
            fs::read_to_string(crate::repo_root().join("Cargo.lock")).expect("the lockfile reads");
        let named = format!("name = \"{EMBER_SDK}\"");
        let scanned: Vec<String> = lockfile
            .split("[[package]]")
            .filter(|block| block.lines().any(|line| line.trim() == named))
            .filter_map(|block| {
                block
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("version = "))
            })
            .map(|value| value.trim_matches('"').to_string())
            .collect();
        assert_eq!(scanned.len(), 1, "the lockfile records {scanned:?}");

        let locked = ember_locked(&lockfile).expect("the lockfile records the sdk");
        assert_eq!(
            locked.version, scanned[0],
            "the reader and the lockfile disagree"
        );
        assert!(
            is_release(&locked.version),
            "the sdk resolves to {}, which is not one release",
            locked.version
        );
        assert_eq!(
            ember_release(&lockfile).expect("the committed lockfile names a release"),
            format!("v{}", scanned[0])
        );
    }

    // ///// The loader /////

    /// A loader whose bytes do not hash to what its digest file records is
    /// refused, and the refusal names both digests.
    #[test]
    fn a_loader_that_fails_its_digest_is_refused() {
        let home = sandbox("mismatch");
        let wrong = "0".repeat(64);
        let loader = stage_loader(home.path(), LOADER_BYTES, Some(&wrong));

        let staged = Local::new(loader.clone())
            .stage(home.path())
            .expect("a local loader stages");
        let err = verified(&staged).expect_err("a wrong digest is refused");

        let said = format!("{err:#}");
        assert!(
            said.contains(&wrong),
            "the refusal hides the recorded digest: {said}"
        );
        assert!(
            said.contains(&digest(LOADER_BYTES)),
            "the refusal hides the digest it found: {said}"
        );
    }

    /// A loader path that names nothing is refused by name, rather than
    /// producing an archive with no loader in it.
    #[test]
    fn a_loader_path_that_names_nothing_is_refused() {
        let home = sandbox("absent");
        let loader = home.path().join("nowhere").join(LOADER);

        let staged = Local::new(loader.clone())
            .stage(home.path())
            .expect("a local loader stages");
        let err = verified(&staged).expect_err("an absent loader is refused");

        assert!(
            format!("{err:#}").contains(&loader.display().to_string()),
            "the refusal does not name the path"
        );
    }

    /// A loader with no digest file beside it is refused. The digest is the
    /// whole reason the loader is not rebuilt here, so a missing one is not a
    /// step to skip.
    #[test]
    fn a_loader_with_no_digest_file_beside_it_is_refused() {
        let home = sandbox("undigested");
        let loader = home.path().join(LOADER);
        put(&loader, LOADER_BYTES);

        let staged = Local::new(loader).stage(home.path()).expect("stages");
        let err = verified(&staged).expect_err("a missing digest file is refused");

        let expected = home.path().join(SUMS);
        assert_eq!(
            staged.sums, expected,
            "the digest file is looked for elsewhere"
        );
        assert!(
            format!("{err:#}").contains(&expected.display().to_string()),
            "the refusal does not name where the digest file was looked for: {err:#}"
        );
    }

    /// A digest file that records other names but not the loader is refused,
    /// rather than passing on another asset's digest.
    #[test]
    fn a_digest_file_that_skips_the_loader_is_refused() {
        let home = sandbox("unrecorded");
        let loader = home.path().join(LOADER);
        put(&loader, LOADER_BYTES);
        put(
            &home.path().join(SUMS),
            sums_line(&digest(LOADER_BYTES), "something-else.dll").as_bytes(),
        );

        let staged = Local::new(loader).stage(home.path()).expect("stages");
        let err = verified(&staged).expect_err("an unrecorded loader is refused");

        assert!(
            format!("{err:#}").contains("records no digest"),
            "got {err:#}"
        );
    }

    /// A loader under any other name is refused, because the archive carries
    /// whatever it is given in the loader's slot. The digest check cannot catch
    /// this: it holds the file to its own recorded digest, under its own name.
    #[test]
    fn a_local_loader_under_another_name_is_refused() {
        let home = sandbox("misnamed");
        let loader = home.path().join("anything-at-all.dll");
        put(&loader, LOADER_BYTES);
        put(
            &home.path().join(SUMS),
            sums_line(&digest(LOADER_BYTES), "anything-at-all.dll").as_bytes(),
        );

        let err = Local::new(loader)
            .stage(home.path())
            .expect_err("a loader under another name is refused");

        let said = format!("{err:#}");
        assert!(said.contains("anything-at-all.dll"), "got {said}");
        assert!(
            said.contains(LOADER),
            "the refusal does not say what to name it: {said}"
        );
    }

    /// What each route prints about where the loader came from. A reader of a
    /// release log has the digest check in front of them either way, and this
    /// line is what says what that check proves.
    #[test]
    fn each_route_says_where_its_loader_came_from() {
        let runner = BuildingRunner {
            artifact: PathBuf::from("Z:/nowhere"),
            bytes: None,
            ran: RefCell::new(Vec::new()),
        };
        let downloaded = Release::new(&runner, "v0.4.2".to_string()).origin();
        assert!(downloaded.contains(EMBER), "got {downloaded}");
        assert!(downloaded.contains("v0.4.2"), "got {downloaded}");

        let local = Local::new(PathBuf::from("Z:/ember/POWRPROF.dll")).origin();
        assert!(local.contains("POWRPROF.dll"), "got {local}");
        assert!(
            local.contains(SUMS) && !local.contains(EMBER),
            "the local route reads as a release download: {local}"
        );
    }

    // ///// The commands this runs /////

    /// The build names the shipped target, the release profile and the
    /// lockfile, and it names its own target directory so the artifact it reads
    /// is the artifact cargo wrote.
    ///
    /// The triple is spelled out rather than taken from the constant, so a
    /// constant that drifts fails here instead of agreeing with itself.
    #[test]
    fn the_build_names_the_shipped_target_and_its_own_target_directory() {
        let target = PathBuf::from("Z:/checkout/target");
        let command = build_command("private-chests", &target);

        assert_eq!(command[0], "cargo");
        for expected in [
            "--release",
            "--locked",
            "--lib",
            "--target",
            "x86_64-pc-windows-msvc",
        ] {
            assert!(
                command.iter().any(|word| word == expected),
                "got {command:?}"
            );
        }
        let named = command
            .windows(2)
            .find(|pair| pair[0] == "--target-dir")
            .map(|pair| pair[1].clone());
        assert_eq!(named, Some(target.to_string_lossy().into_owned()));

        let built = artifact(&target, "private_chests.dll");
        assert!(built.ends_with("private_chests.dll"), "got {built:?}");
        assert!(
            built.to_string_lossy().contains(TARGET),
            "the artifact is read from the host directory: {built:?}"
        );
    }

    /// The download names the release by tag, names Ember's repository, and
    /// takes both assets. A download of the library alone would leave the
    /// digest check with nothing to read.
    ///
    /// The repository and both asset names are spelled out rather than taken
    /// from the constants, so a constant that drifts fails here instead of
    /// agreeing with itself.
    #[test]
    fn the_download_names_the_release_and_takes_both_assets() {
        let into = PathBuf::from("Z:/staging");
        let command = Release::command("v0.4.2", &into);

        assert_eq!(&command[..3], ["gh", "release", "download"]);
        assert_eq!(command[3], "v0.4.2");
        let value = |flag: &str| {
            command
                .windows(2)
                .find(|pair| pair[0] == flag)
                .map(|pair| pair[1].clone())
        };
        assert_eq!(
            value("--repo"),
            Some("zachthedev/enshrouded-ember".to_string())
        );
        assert_eq!(value("--dir"), Some(into.to_string_lossy().into_owned()));
        let patterns: Vec<&String> = command
            .windows(2)
            .filter(|pair| pair[0] == "--pattern")
            .map(|pair| &pair[1])
            .collect();
        assert_eq!(patterns, vec!["POWRPROF.dll", "SHA256SUMS"]);
    }

    // ///// The command end to end /////

    /// A `Runner` that writes the artifact a build would have written, so the
    /// case reaches the archive rather than a cargo invocation.
    ///
    /// A nested cargo build would wait on the outer run's lock over the same
    /// target directory, and building the mod in its own directory is a release
    /// build of the whole graph per case.
    struct BuildingRunner {
        /// Where the build is expected to leave the library.
        artifact: PathBuf,
        /// The bytes to leave there, or none to leave nothing.
        bytes: Option<Vec<u8>>,
        /// Every command this runner was asked to run, in order.
        ran: RefCell<Vec<String>>,
    }

    impl Runner for BuildingRunner {
        fn capture(&self, _command: &[&str]) -> Option<String> {
            None
        }

        fn capture_any(&self, _command: &[&str]) -> Option<String> {
            None
        }

        fn run(&self, command: &[&str], _env: &[(&str, &str)]) -> io::Result<Exit> {
            self.ran.borrow_mut().push(command.join(" "));
            if let Some(bytes) = &self.bytes {
                put(&self.artifact, bytes);
            }
            Ok(Exit::Ok)
        }

        fn read_file(&self, _relative: &str) -> Option<String> {
            None
        }

        fn resolve(&self, _tool: &str) -> Option<PathBuf> {
            None
        }
    }

    /// The command builds the mod, holds a local loader to its digest, and
    /// writes one archive and one digest file, with the loader download the
    /// only substituted step.
    #[test]
    fn the_command_builds_one_archive_from_a_local_loader() {
        let home = sandbox("end-to-end");
        let root = workspace(home.path());
        let loader = stage_loader(&home.path().join("ember"), LOADER_BYTES, None);
        let out = home.path().join("out");
        let runner = BuildingRunner {
            artifact: artifact(&root.join("target"), "private_chests.dll"),
            bytes: Some(LIBRARY_BYTES.to_vec()),
            ran: RefCell::new(Vec::new()),
        };
        let request = Request {
            subject: "private-chests".to_string(),
            tag: "private-chests-v0.1.0".to_string(),
            out: out.clone(),
            loader: Some(loader),
        };

        let mut printed = Vec::new();
        let written =
            run(&runner, &root, &request, &mut printed).expect("the command builds a bundle");

        assert_eq!(
            entries(&written.archive),
            vec![
                LOADER.to_string(),
                format!("{MODS}/private-chests/private_chests.dll"),
            ]
        );
        assert_eq!(entry_bytes(&written.archive, LOADER), LOADER_BYTES);
        assert_eq!(
            written.archive,
            out.join("private-chests-v0.1.0.zip"),
            "the archive is not named for the tag"
        );
        let ran = runner.ran.borrow().clone();
        assert_eq!(ran.len(), 1, "it ran more than the build: {ran:?}");
        assert!(
            ran[0].contains(TARGET),
            "the build missed the target: {ran:?}"
        );
        assert!(
            !ran.iter().any(|line| line.starts_with("gh ")),
            "it reached the network with a loader on disk: {ran:?}"
        );
    }

    /// An occupied output directory is refused before the build runs.
    ///
    /// The refusal reads only the directory, so paying for a release
    /// cross-compile, and on the download route a fetch, before reporting it is
    /// cost a re-run after a failure does not have to carry.
    #[test]
    fn an_occupied_output_directory_is_refused_before_anything_is_built() {
        let home = sandbox("occupied-run");
        let root = workspace(home.path());
        let loader = stage_loader(&home.path().join("ember"), LOADER_BYTES, None);
        let out = home.path().join("out");
        put(&out.join("an-earlier-run.zip"), b"a previous bundle");
        let runner = BuildingRunner {
            artifact: artifact(&root.join("target"), "private_chests.dll"),
            bytes: Some(LIBRARY_BYTES.to_vec()),
            ran: RefCell::new(Vec::new()),
        };
        let request = Request {
            subject: "private-chests".to_string(),
            tag: "private-chests-v0.1.0".to_string(),
            out,
            loader: Some(loader),
        };

        let mut printed = Vec::new();
        let err = run(&runner, &root, &request, &mut printed)
            .expect_err("an occupied output directory is refused");

        assert!(
            format!("{err:#}").contains("an-earlier-run.zip"),
            "got {err:#}"
        );
        assert!(
            runner.ran.borrow().is_empty(),
            "it built before refusing: {:?}",
            runner.ran.borrow()
        );
    }

    /// The download route refuses an sdk that comes from a directory, and
    /// refuses it before it builds anything, rather than downloading a loader
    /// built from code the mod was not compiled against.
    ///
    /// The lockfile is written into the sandbox rather than taken from the copy
    /// `workspace` lays down: the case has to present a directory whatever this
    /// repository's own lockfile records. The version is one Ember released, so
    /// the missing source is the only thing left to refuse.
    #[test]
    fn the_download_route_refuses_an_sdk_from_a_directory() {
        let home = sandbox("directory");
        let root = workspace(home.path());
        put(
            &root.join("Cargo.lock"),
            format!("[[package]]\nname = \"{EMBER_SDK}\"\nversion = \"0.1.0\"\n").as_bytes(),
        );
        let runner = BuildingRunner {
            artifact: artifact(&root.join("target"), "private_chests.dll"),
            bytes: Some(LIBRARY_BYTES.to_vec()),
            ran: RefCell::new(Vec::new()),
        };
        let request = Request {
            subject: "private-chests".to_string(),
            tag: "private-chests-v0.1.0".to_string(),
            out: home.path().join("out"),
            loader: None,
        };

        let mut printed = Vec::new();
        let err = run(&runner, &root, &request, &mut printed)
            .expect_err("an sdk from a directory is refused");

        let said = format!("{err:#}");
        assert!(said.contains("a directory"), "got {said}");
        assert!(
            said.contains("--ember-loader"),
            "the refusal does not say what to do instead: {said}"
        );
        assert!(
            runner.ran.borrow().is_empty(),
            "it ran {:?} before refusing",
            runner.ran.borrow()
        );
    }

    /// Every refusal the command makes before it builds anything, each named by
    /// what a caller got wrong.
    #[test]
    fn the_command_refuses_a_request_it_cannot_honor() {
        let cases: Vec<(&str, &str, &str, &str)> = vec![
            (
                "a mod this workspace does not hold",
                "not-a-mod",
                "not-a-mod-v0.1.0",
                "holds no mod named not-a-mod",
            ),
            (
                "a tag that is not a release tag",
                "private-chests",
                "private-chests-0.1.0",
                "<package>-v<version>",
            ),
            (
                "a tag naming another package",
                "private-chests",
                "other-mod-v0.1.0",
                "and the bundle is for private-chests",
            ),
            (
                "a tag carrying another version",
                "private-chests",
                "private-chests-v9.9.9",
                "carries 9.9.9",
            ),
        ];

        for (label, requested, tag, expected) in cases {
            let home = sandbox("refused");
            let root = workspace(home.path());
            let loader = stage_loader(&home.path().join("ember"), LOADER_BYTES, None);
            let out = home.path().join("out");
            let runner = BuildingRunner {
                artifact: home.path().join("never"),
                bytes: None,
                ran: RefCell::new(Vec::new()),
            };
            let request = Request {
                subject: requested.to_string(),
                tag: tag.to_string(),
                out: out.clone(),
                loader: Some(loader),
            };

            let mut printed = Vec::new();
            let err = run(&runner, &root, &request, &mut printed)
                .expect_err(&format!("{label}: it built a bundle anyway"));

            let said = format!("{err:#}");
            assert!(said.contains(expected), "{label}: got {said}");
            assert!(
                runner.ran.borrow().is_empty(),
                "{label}: it ran {:?} before refusing",
                runner.ran.borrow()
            );
            assert!(!out.exists(), "{label}: it wrote into the output directory");
        }
    }
}
