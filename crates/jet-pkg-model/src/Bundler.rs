//! Deterministic package bundle plans and layout artifacts.
//!
//! This module owns the data side of desktop and game packaging.  It does not
//! invoke platform packers, signers, launchers, or update services.  A caller
//! supplies an explicit target and executable bytes; the resulting plan is a
//! complete, typed file layout that a later platform adapter may materialize.
//!
//! The same model is used for desktop and game packages.  `BundleKind` records
//! that intent for receipts without creating a second game-specific bundler.

use crate::SHA256;
use jet_foundation::PerformanceBudget::CanonicalJson;
use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Write as _;

pub const PACKAGE_BUNDLE_SCHEMA_VERSION: u32 = 1;
pub const PACKAGE_BUNDLE_FORMAT: &str = "jet.package.bundle";
pub const PACKAGE_BUNDLE_RECEIPT_SCHEMA: &str = "jet.package.receipt";
pub const PACKAGE_UPDATE_MANIFEST_SCHEMA: &str = "jet.package.update";

/// The explicit package target.  No value is inferred from the host process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BundleTarget {
    LinuxAppImage,
    MacOSApp,
    WindowsMsix,
}

impl BundleTarget {
    pub const ALL: [Self; 3] = [Self::LinuxAppImage, Self::MacOSApp, Self::WindowsMsix];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LinuxAppImage => "linux-appimage",
            Self::MacOSApp => "macos-app",
            Self::WindowsMsix => "windows-msix",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "linux-appimage" | "linux" | "appimage" => Some(Self::LinuxAppImage),
            "macos-app" | "macos" | "mac" | "app" => Some(Self::MacOSApp),
            "windows-msix" | "windows" | "msix" => Some(Self::WindowsMsix),
            _ => None,
        }
    }

    pub fn artifact_suffix(self) -> &'static str {
        match self {
            Self::LinuxAppImage => ".AppImage",
            Self::MacOSApp => ".app",
            Self::WindowsMsix => ".msix",
        }
    }
}

/// Whether the shared bundle is for a desktop application or a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BundleKind {
    Desktop,
    Game,
}

impl BundleKind {

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Game => "game",
        }
    }
}
impl Default for BundleKind {
    fn default() -> Self {
        Self::Desktop
    }
}

/// Input executable supplied by the compiler/build adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleExecutable {
    /// File name used inside the target layout.
    pub name: String,
    /// Source path recorded in the receipt; it is never resolved by this model.
    pub source: String,
    /// Exact executable bytes to place in the layout.
    pub bytes: Vec<u8>,
}

impl BundleExecutable {
    pub fn new(
        name: impl Into<String>,
        source: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            bytes: bytes.into(),
        }
    }

    pub fn digest(&self) -> String {
        SHA256::sha256_hex(&self.bytes)
    }
}

/// Icon payload and its deterministic target-independent identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleIcon {
    pub name: String,
    pub format: IconFormat,
    pub size: u32,
    /// Optional source path for receipt provenance.  The bytes remain the
    /// authority for the emitted artifact.
    pub source: Option<String>,
    pub bytes: Vec<u8>,
}

impl BundleIcon {
    pub fn new(
        name: impl Into<String>,
        format: IconFormat,
        size: u32,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            name: name.into(),
            format,
            size,
            source: None,
            bytes: bytes.into(),
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn digest(&self) -> String {
        SHA256::sha256_hex(&self.bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IconFormat {
    Png,
    Icns,
    Ico,
    Svg,
}

impl IconFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Icns => "icns",
            Self::Ico => "ico",
            Self::Svg => "svg",
        }
    }

    pub fn extension(self) -> &'static str {
        self.as_str()
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().trim_start_matches('.').to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "icns" => Some(Self::Icns),
            "ico" => Some(Self::Ico),
            "svg" => Some(Self::Svg),
            _ => None,
        }
    }
}

/// A signer/verification adapter input.  The adapter owns the actual command.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SigningHookInput {
    pub path: String,
    pub digest: String,
}

impl SigningHookInput {
    pub fn new(path: impl Into<String>, digest: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            digest: digest.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SigningPhase {
    PreSign,
    PostSign,
    Verify,
}

impl SigningPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreSign => "pre-sign",
            Self::PostSign => "post-sign",
            Self::Verify => "verify",
        }
    }
}

/// A typed signing hook declaration.  No hook is executed by this module.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SigningHook {
    pub name: String,
    pub phase: SigningPhase,
    pub program: String,
    pub args: Vec<String>,
    pub inputs: Vec<SigningHookInput>,
}

impl SigningHook {
    pub fn new(
        name: impl Into<String>,
        phase: SigningPhase,
        program: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            phase,
            program: program.into(),
            args: Vec::new(),
            inputs: Vec::new(),
        }
    }
}

/// User-declared updater endpoint.  The manifest's artifact and payload facts
/// are filled from the exact executable payload by the checked plan; the
/// receipt carries the final materialized artifact digest.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UpdaterSpec {
    pub channel: String,
    pub url: String,
    pub signature_url: Option<String>,
    pub release_notes_url: Option<String>,
}

impl UpdaterSpec {
    pub fn new(channel: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            channel: channel.into(),
            url: url.into(),
            signature_url: None,
            release_notes_url: None,
        }
    }
}

/// All inputs needed to produce one target-specific bundle layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleSpec {
    pub kind: BundleKind,
    pub target: BundleTarget,
    pub package: String,
    pub version: String,
    pub executable: BundleExecutable,
    pub metadata: BundleMetadata,
    pub icons: Vec<BundleIcon>,
    pub signing_hooks: Vec<SigningHook>,
    pub updater: Option<UpdaterSpec>,
    pub game: Option<GameBundleSpec>,
}

impl BundleSpec {
    pub fn new(
        kind: BundleKind,
        target: BundleTarget,
        package: impl Into<String>,
        version: impl Into<String>,
        executable: BundleExecutable,
        metadata: BundleMetadata,
    ) -> Self {
        Self {
            kind,
            target,
            package: package.into(),
            version: version.into(),
            executable,
            metadata,
            icons: Vec::new(),
            signing_hooks: Vec::new(),
            updater: None,
            game: None,
        }
    }

    pub fn desktop(
        target: BundleTarget,
        package: impl Into<String>,
        version: impl Into<String>,
        executable: BundleExecutable,
        metadata: BundleMetadata,
    ) -> Self {
        Self::new(
            BundleKind::Desktop,
            target,
            package,
            version,
            executable,
            metadata,
        )
    }

    pub fn game(
        target: BundleTarget,
        package: impl Into<String>,
        version: impl Into<String>,
        executable: BundleExecutable,
        metadata: BundleMetadata,
    ) -> Self {
        Self::new(
            BundleKind::Game,
            target,
            package,
            version,
            executable,
            metadata,
        )
    }
}

/// Human-facing and platform-facing metadata.  `identifier` is the stable
/// package identity; display text can contain spaces and Unicode.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BundleMetadata {
    pub identifier: String,
    pub display_name: String,
    pub publisher: String,
    pub description: String,
    pub categories: Vec<String>,
}

impl BundleMetadata {
    pub fn new(identifier: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
            display_name: display_name.into(),
            publisher: String::new(),
            description: String::new(),
            categories: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BundleFileKind {
    Executable,
    Launcher,
    Metadata,
    Manifest,
    Icon,
    Updater,
    ContentTypes,
    BlockMap,
    GameManifest,
    GameAsset,
}

impl BundleFileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::Launcher => "launcher",
            Self::Metadata => "metadata",
            Self::Manifest => "manifest",
            Self::Icon => "icon",
            Self::Updater => "updater",
            Self::ContentTypes => "content-types",
            Self::BlockMap => "block-map",
            Self::GameManifest => "game-manifest",
            Self::GameAsset => "game-asset",
        }
    }
}
 
/// One deterministic asset payload carried by a game bundle plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameBundleAsset {
    pub path: String,
    pub mode: u32,
    pub bytes: Vec<u8>,
}

impl GameBundleAsset {
    pub fn new(path: impl Into<String>, mode: u32, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            mode,
            bytes: bytes.into(),
        }
    }

    pub fn digest(&self) -> String {
        SHA256::sha256_hex(&self.bytes)
    }
}


/// Canonical bounds shared by the package receipt and the generated game
/// crash reporter.  The reporter keeps these values in memory and persists at
/// most one latest report; upload remains disabled unless a transport is
/// explicitly added by a later adapter.
pub const GAME_CRASH_MAX_STACKS: u32 = 32;
pub const GAME_CRASH_MAX_LOGS: u32 = 256;
pub const GAME_CRASH_MAX_PERSISTED: u32 = 1;
pub const GAME_CRASH_REPORTER_ABI: &str = "jet-game-crash-reporter-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameCrashReporterSpec {
    pub installation: String,
    pub upload_policy: String,
    pub routing: String,
    pub memory_stacks: u32,
    pub memory_logs: u32,
    pub retention_mode: String,
    pub retention_path: String,
    pub retention_limit: u32,
}

impl GameCrashReporterSpec {
    pub fn not_installed() -> Self {
        Self {
            installation: "not-installed".into(),
            upload_policy: "never".into(),
            routing: "local".into(),
            memory_stacks: GAME_CRASH_MAX_STACKS,
            memory_logs: GAME_CRASH_MAX_LOGS,
            retention_mode: "none".into(),
            retention_path: String::new(),
            retention_limit: 0,
        }
    }

    pub fn planned() -> Self {
        Self {
            installation: "planned".into(),
            upload_policy: "never".into(),
            routing: "local".into(),
            memory_stacks: GAME_CRASH_MAX_STACKS,
            memory_logs: GAME_CRASH_MAX_LOGS,
            retention_mode: "bounded-persisted".into(),
            retention_path: "crash-reports".into(),
            retention_limit: GAME_CRASH_MAX_PERSISTED,
        }
    }

    pub fn installed() -> Self {
        Self {
            installation: "installed".into(),
            upload_policy: "never".into(),
            routing: "local".into(),
            memory_stacks: GAME_CRASH_MAX_STACKS,
            memory_logs: GAME_CRASH_MAX_LOGS,
            retention_mode: "bounded-persisted".into(),
            retention_path: "crash-reports".into(),
            retention_limit: GAME_CRASH_MAX_PERSISTED,
        }
    }
}

/// Game-only facts attached to the shared package plan.  The package command
/// computes these facts; the bundler owns their canonical layout and receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameBundleSpec {
    pub profile: String,
    pub export_preset: String,
    pub source_revision: String,
    pub build_identity: String,
    pub backend: String,
    pub renderer: String,
    pub cook_mode: String,
    pub development_stripping: bool,
    pub crash_reporter: String,
    pub crash_consent: String,
    pub crash_runtime: GameCrashReporterSpec,
    pub assets: Vec<GameBundleAsset>,
}

impl GameBundleSpec {
    pub fn new(
        profile: impl Into<String>,
        export_preset: impl Into<String>,
        source_revision: impl Into<String>,
        backend: impl Into<String>,
        renderer: impl Into<String>,
        cook_mode: impl Into<String>,
    ) -> Self {
        Self {
            profile: profile.into(),
            export_preset: export_preset.into(),
            source_revision: source_revision.into(),
            build_identity: "planned".into(),
            backend: backend.into(),
            renderer: renderer.into(),
            cook_mode: cook_mode.into(),
            development_stripping: false,
            crash_reporter: "off".into(),
            crash_consent: "not-requested".into(),
            crash_runtime: GameCrashReporterSpec::not_installed(),
            assets: Vec::new(),
        }
    }
}



/// One exact file in a plan or emitted artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleFile {
    pub path: String,
    pub kind: BundleFileKind,
    pub mode: u32,
    pub bytes: Vec<u8>,
}

impl BundleFile {
    fn new(path: impl Into<String>, kind: BundleFileKind, mode: u32, bytes: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            kind,
            mode,
            bytes,
        }
    }

    pub fn digest(&self) -> String {
        SHA256::sha256_hex(&self.bytes)
    }
}

/// The exact launch paths a later adapter must preserve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchDescriptor {
    pub launcher: String,
    pub executable: String,
    pub arguments_passthrough: bool,
}

impl LaunchDescriptor {
    fn new(launcher: impl Into<String>, executable: impl Into<String>) -> Self {
        Self {
            launcher: launcher.into(),
            executable: executable.into(),
            arguments_passthrough: true,
        }
    }
}

/// A complete, deterministic target layout.  It contains no host-derived
/// values and can be handed to a platform adapter without re-planning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundlePlan {
    pub schema_version: u32,
    pub kind: BundleKind,
    pub target: BundleTarget,
    pub package: String,
    pub version: String,
    /// Source path of the executable input, retained for receipt provenance.
    pub executable_source: String,
    pub artifact_name: String,
    pub metadata: BundleMetadata,
    pub files: Vec<BundleFile>,
    pub launch: LaunchDescriptor,
    pub inputs: Vec<BundleInputFact>,
    pub signing_hooks: Vec<SigningHook>,
    pub updater: Option<UpdaterManifest>,
    pub game: Option<GameBundleSpec>,
    pub identity: String,
}

impl BundlePlan {
    pub fn new(spec: &BundleSpec) -> Result<Self, BundleError> {
        Self::from_spec(spec)
    }

    pub fn from_spec(spec: &BundleSpec) -> Result<Self, BundleError> {
        let mut normalized_spec = spec.clone();
        normalized_spec.version = if normalized_spec.target == BundleTarget::WindowsMsix {
            windows_version(&normalized_spec.version)?
        } else {
            normalized_spec.version.trim().to_owned()
        };
        let spec = &normalized_spec;
        validate_spec(spec)?;
        let mut icons = spec.icons.clone();
        icons.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then(left.format.cmp(&right.format))
                .then(left.size.cmp(&right.size))
                .then(left.digest().cmp(&right.digest()))
        });
        let mut hooks = spec.signing_hooks.clone();
        hooks.sort();
        let mut categories = spec.metadata.categories.clone();
        categories.sort();

        let mut metadata = spec.metadata.clone();
        metadata.categories = categories;
        let executable_digest = spec.executable.digest();
        let mut game = spec.game.clone();
        if let Some(game) = game.as_mut() {
            game.assets.sort_by(|left, right| {
                left.path
                    .cmp(&right.path)
                    .then(left.digest().cmp(&right.digest()))
            });
        }
        let metadata_json = bundle_metadata_json(spec, &metadata, &executable_digest, &icons, &hooks);
        let mut input_facts = vec![BundleInputFact {
            role: "executable".into(),
            path: spec.executable.source.clone(),
            digest: executable_digest.clone(),
            bytes: spec.executable.bytes.len() as u64,
        }];
        for icon in &icons {
            if let Some(source) = icon.source.as_ref() {
                input_facts.push(BundleInputFact {
                    role: "icon".into(),
                    path: source.clone(),
                    digest: icon.digest(),
                    bytes: icon.bytes.len() as u64,
                });
            }
        }
        if let Some(game) = game.as_ref() {
            input_facts.push(BundleInputFact {
                role: "game-source".into(),
                path: "<project>".into(),
                digest: game.source_revision.clone(),
                bytes: 0,
            });
            for asset in &game.assets {
                input_facts.push(BundleInputFact {
                    role: "game-asset".into(),
                    path: asset.path.clone(),
                    digest: asset.digest(),
                    bytes: asset.bytes.len() as u64,
                });
            }
        }
        input_facts.sort_by(|left, right| left.role.cmp(&right.role).then(left.path.cmp(&right.path)));
        let metadata_bytes = metadata_json.bytes();
        let mut files = Vec::new();
        let (artifact_name, launch) = match spec.target {
            BundleTarget::LinuxAppImage => {
                let executable = format!("usr/bin/{}", spec.executable.name);
                files.push(BundleFile::new(
                    "AppRun",
                    BundleFileKind::Launcher,
                    0o755,
                    linux_launcher(&spec.executable.name),
                ));
                files.push(BundleFile::new(
                    executable.clone(),
                    BundleFileKind::Executable,
                    0o755,
                    spec.executable.bytes.clone(),
                ));
                let icon_name = icons.first().map(|icon| icon.name.as_str());
                files.push(BundleFile::new(
                    format!("usr/share/applications/{}.desktop", spec.metadata.identifier),
                    BundleFileKind::Manifest,
                    0o644,
                    linux_desktop_file(&metadata, icon_name),
                ));
                files.push(BundleFile::new(
                    format!("usr/share/{}/jet-bundle.json", spec.metadata.identifier),
                    BundleFileKind::Metadata,
                    0o644,
                    metadata_bytes.clone(),
                ));
                for icon in &icons {
                    let path = format!(
                        "usr/share/icons/hicolor/{}x{}/apps/{}.{}",
                        icon.size,
                        icon.size,
                        icon.name,
                        icon.format.extension()
                    );
                    files.push(BundleFile::new(
                        path,
                        BundleFileKind::Icon,
                        0o644,
                        icon.bytes.clone(),
                    ));
                }
                if let Some(icon) = icons.first() {
                    files.push(BundleFile::new(
                        ".DirIcon",
                        BundleFileKind::Icon,
                        0o644,
                        icon.bytes.clone(),
                    ));
                }
                let update_path = format!("usr/share/{}/jet-update.json", spec.metadata.identifier);
                if let Some(updater) = spec.updater.as_ref() {
                    let manifest = updater_manifest(spec, updater, &executable_digest);
                    files.push(BundleFile::new(
                        update_path,
                        BundleFileKind::Updater,
                        0o644,
                        manifest.json_bytes(),
                    ));
                }
                (
                    format!("{}{}", spec.metadata.identifier, spec.target.artifact_suffix()),
                    LaunchDescriptor::new("AppRun", executable),
                )
            },
            BundleTarget::MacOSApp => {
                let root = format!("{}.app", spec.metadata.identifier);
                let executable = format!("{root}/Contents/MacOS/{}", spec.executable.name);
                files.push(BundleFile::new(
                    executable.clone(),
                    BundleFileKind::Executable,
                    0o755,
                    spec.executable.bytes.clone(),
                ));
                let icon_name = icons.first().map(|icon| format!("{}.{}", icon.name, icon.format.extension()));
                files.push(BundleFile::new(
                    format!("{root}/Contents/Info.plist"),
                    BundleFileKind::Manifest,
                    0o644,
                    mac_info_plist(spec, icon_name.as_deref()),
                ));
                files.push(BundleFile::new(
                    format!("{root}/Contents/Resources/jet-bundle.json"),
                    BundleFileKind::Metadata,
                    0o644,
                    metadata_bytes.clone(),
                ));
                for icon in &icons {
                    files.push(BundleFile::new(
                        format!(
                            "{root}/Contents/Resources/{}.{}",
                            icon.name,
                            icon.format.extension()
                        ),
                        BundleFileKind::Icon,
                        0o644,
                        icon.bytes.clone(),
                    ));
                }
                if let Some(updater) = spec.updater.as_ref() {
                    let manifest = updater_manifest(spec, updater, &executable_digest);
                    files.push(BundleFile::new(
                        format!("{root}/Contents/Resources/jet-update.json"),
                        BundleFileKind::Updater,
                        0o644,
                        manifest.json_bytes(),
                    ));
                }
                (
                    format!("{}{}", spec.metadata.identifier, spec.target.artifact_suffix()),
                    LaunchDescriptor::new(
                        format!("{root}/Contents/MacOS/{}", spec.executable.name),
                        executable,
                    ),
                )
            },
            BundleTarget::WindowsMsix => {
                let executable_name = windows_executable_name(&spec.executable.name);
                let executable = executable_name.clone();
                files.push(BundleFile::new(
                    executable.clone(),
                    BundleFileKind::Executable,
                    0o755,
                    spec.executable.bytes.clone(),
                ));
                let logo = icons.first().map(|icon| {
                    format!("Assets/{}.{}", icon.name, icon.format.extension())
                });
                files.push(BundleFile::new(
                    "AppxManifest.xml",
                    BundleFileKind::Manifest,
                    0o644,
                    windows_manifest(spec, &executable, logo.as_deref())?,
                ));
                let content_types = windows_content_types(&files);
                files.push(BundleFile::new(
                    "[Content_Types].xml",
                    BundleFileKind::ContentTypes,
                    0o644,
                    content_types,
                ));
                files.push(BundleFile::new(
                    "jet/jet-bundle.json",
                    BundleFileKind::Metadata,
                    0o644,
                    metadata_bytes,
                ));
                for icon in &icons {
                    files.push(BundleFile::new(
                        format!("Assets/{}.{}", icon.name, icon.format.extension()),
                        BundleFileKind::Icon,
                        0o644,
                        icon.bytes.clone(),
                    ));
                }
                if let Some(updater) = spec.updater.as_ref() {
                    let manifest = updater_manifest(spec, updater, &executable_digest);
                    files.push(BundleFile::new(
                        "jet/jet-update.json",
                        BundleFileKind::Updater,
                        0o644,
                        manifest.json_bytes(),
                    ));
                }
                (
                    format!("{}{}", spec.metadata.identifier, spec.target.artifact_suffix()),
                    LaunchDescriptor::new(executable.clone(), executable),
                )
            }
        };
        if let Some(game) = game.as_ref() {
            files.push(BundleFile::new(
                game_manifest_path(spec),
                BundleFileKind::GameManifest,
                0o644,
                game_manifest_json(spec, game).bytes(),
            ));
            for asset in &game.assets {
                files.push(BundleFile::new(
                    game_asset_path(spec, &asset.path),
                    BundleFileKind::GameAsset,
                    asset.mode,
                    asset.bytes.clone(),
                ));
            }
        }
        if spec.target == BundleTarget::WindowsMsix {
            let content_types = windows_content_types(&files);
            if let Some(file) = files.iter_mut().find(|file| file.kind == BundleFileKind::ContentTypes) {
                file.bytes = content_types;
            }
            let block_map = windows_block_map(&files);
            files.push(BundleFile::new(
                "AppxBlockMap.xml",
                BundleFileKind::BlockMap,
                0o644,
                block_map,
            ));
        }

        let mut paths = BTreeSet::new();
        for file in &files {
            if !paths.insert(file.path.clone()) {
                return Err(BundleError::DuplicatePath(file.path.clone()));
            }
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let mut plan = Self {
            schema_version: PACKAGE_BUNDLE_SCHEMA_VERSION,
            kind: spec.kind,
            target: spec.target,
            package: spec.package.clone(),
            version: spec.version.clone(),
            executable_source: spec.executable.source.clone(),
            artifact_name,
            metadata,
            files,
            launch,
            inputs: input_facts,
            signing_hooks: hooks,
            updater: spec
                .updater
                .as_ref()
                .map(|updater| updater_manifest(spec, updater, &executable_digest)),
            game: game.clone(),
            identity: String::new(),
        };
        plan.identity = format!("jet-bundle-plan-sha256:{}", SHA256::sha256_hex(&plan.canonical_bytes()));
        Ok(plan)
    }

    /// Return the deterministic logical artifact view used by callers that do
    /// not have a platform transport.  The package command uses
    /// [`Self::materialized_artifact`] after writing the target artifact.
    pub fn emit(&self) -> BundleArtifact {
        self.materialized_artifact(self.logical_artifact_sha256(), self.logical_artifact_bytes())
    }

    /// Digest of the checked plan's sorted file set.  Platform adapters may
    /// replace this with the digest of their actual transport bytes.
    pub fn logical_artifact_sha256(&self) -> String {
        artifact_digest(&self.files)
    }

    /// Sum of the checked plan's file payloads.
    pub fn logical_artifact_bytes(&self) -> u64 {
        self.files
            .iter()
            .map(|file| file.bytes.len() as u64)
            .sum()
    }

    /// Build an artifact and typed receipt from an adapter's materialized
    /// digest and byte count.  The plan remains the semantic authority; the
    /// adapter contributes only transport facts.
    pub fn materialized_artifact(
        &self,
        artifact_sha256: impl Into<String>,
        artifact_bytes: u64,
    ) -> BundleArtifact {
        let artifact_sha256 = artifact_sha256.into();
        let receipt = BundleReceipt::from_plan_with_bytes(self, &artifact_sha256, artifact_bytes);
        BundleArtifact {
            schema_version: self.schema_version,
            kind: self.kind,
            target: self.target,
            package: self.package.clone(),
            version: self.version.clone(),
            artifact_name: self.artifact_name.clone(),
            files: self.files.clone(),
            launch: self.launch.clone(),
            receipt,
        }
    }

    pub fn artifact(&self) -> BundleArtifact {
        self.emit()
    }

    pub fn file_paths(&self) -> Vec<String> {
        self.files.iter().map(|file| file.path.clone()).collect()
    }

    pub fn identity(&self) -> String {
        let mut identity_plan = self.clone();
        identity_plan.identity.clear();
        format!(
            "jet-bundle-plan-sha256:{}",
            SHA256::sha256_hex(&identity_plan.canonical_bytes())
        )
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut canonical_plan = self.clone();
        canonical_plan.identity.clear();
        // Source paths are receipt provenance, not package meaning.  Keep the
        // role, digest, and byte count in the identity so identical inputs
        // from different checkouts produce one cache key.
        canonical_plan.executable_source.clear();
        for input in &mut canonical_plan.inputs {
            if matches!(input.role.as_str(), "executable" | "icon") {
                input.path.clear();
            }
        }
        bundle_plan_json(&canonical_plan).bytes()
    }

    pub fn json(&self) -> String {
        String::from_utf8(self.to_json().bytes()).expect("canonical bundle JSON is UTF-8")
    }

    pub fn to_json(&self) -> CanonicalJson {
        bundle_plan_json(self)
    }
}

/// Bytes plus receipt facts ready for a platform adapter to materialize.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleArtifact {
    pub schema_version: u32,
    pub kind: BundleKind,
    pub target: BundleTarget,
    pub package: String,
    pub version: String,
    pub artifact_name: String,
    pub files: Vec<BundleFile>,
    pub launch: LaunchDescriptor,
    pub receipt: BundleReceipt,
}

impl BundleArtifact {
    pub fn digest(&self) -> String {
        self.receipt.artifact_sha256.clone()
    }

    pub fn bytes(&self) -> u64 {
        self.receipt.artifact_bytes
    }

    pub fn file_paths(&self) -> Vec<String> {
        self.files.iter().map(|file| file.path.clone()).collect()
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        self.to_json().bytes()
    }

    pub fn json(&self) -> String {
        String::from_utf8(self.canonical_bytes()).expect("canonical bundle artifact JSON is UTF-8")
    }

    pub fn to_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            ("artifact_name".into(), text(&self.artifact_name)),
            ("artifact_sha256".into(), text(&self.digest())),
            ("format".into(), text(PACKAGE_BUNDLE_FORMAT)),
            ("kind".into(), text(self.kind.as_str())),
            ("package".into(), text(&self.package)),
            ("receipt".into(), self.receipt.to_json()),
            ("schema_version".into(), integer(self.schema_version)),
            ("target".into(), text(self.target.as_str())),
            ("version".into(), text(&self.version)),
        ])
        .expect("fixed bundle artifact JSON keys")
    }
}


/// Updater manifest generated from the exact executable payload.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UpdaterManifest {
    pub schema: String,
    pub package: String,
    pub version: String,
    pub target: BundleTarget,
    pub channel: String,
    pub artifact: String,
    pub url: String,
    pub signature_url: Option<String>,
    pub release_notes_url: Option<String>,
    pub payload_sha256: String,
    pub payload_bytes: u64,
}

impl UpdaterManifest {
    pub fn to_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            ("artifact".into(), text(&self.artifact)),
            ("channel".into(), text(&self.channel)),
            ("package".into(), text(&self.package)),
            ("payload_bytes".into(), integer(self.payload_bytes)),
            ("payload_sha256".into(), text(&self.payload_sha256)),
            ("release_notes_url".into(), optional_text(self.release_notes_url.as_deref())),
            ("schema".into(), text(&self.schema)),
            ("signature_url".into(), optional_text(self.signature_url.as_deref())),
            ("target".into(), text(self.target.as_str())),
            ("url".into(), text(&self.url)),
            ("version".into(), text(&self.version)),
        ])
        .expect("fixed updater manifest JSON keys")
    }

    pub fn json_bytes(&self) -> Vec<u8> {
        self.to_json().bytes()
    }

    pub fn json(&self) -> String {
        String::from_utf8(self.json_bytes()).expect("canonical updater JSON is UTF-8")
    }
}

/// Whether the artifact facts describe only the checked plan or a transport
/// produced by a platform adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BundleArtifactStatus {
    Planned,
    Materialized,
}

impl BundleArtifactStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Materialized => "materialized",
        }
    }
}

/// Signing state recorded by an adapter.  A declared hook is only
/// `Requested`; it never implies that a signer ran or produced a signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BundleSigningStatus {
    NotRequested,
    Requested,
    Signed,
    Failed,
}

impl BundleSigningStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRequested => "not-requested",
            Self::Requested => "requested",
            Self::Signed => "signed",
            Self::Failed => "failed",
        }
    }
}

/// Signing execution facts attached after a platform adapter runs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BundleSigningFact {
    pub status: BundleSigningStatus,
    pub tool: Option<String>,
    pub tool_identity: Option<String>,
    pub tool_version: Option<String>,
    pub diagnostic: Option<String>,
}

impl BundleSigningFact {
    pub fn signed() -> Self {
        Self {
            status: BundleSigningStatus::Signed,
            tool: None,
            tool_identity: None,
            tool_version: None,
            diagnostic: None,
        }
    }

    pub fn failed(diagnostic: impl Into<String>) -> Self {
        Self {
            status: BundleSigningStatus::Failed,
            tool: None,
            tool_identity: None,
            tool_version: None,
            diagnostic: Some(diagnostic.into()),
        }
    }

    pub fn with_tool(
        mut self,
        tool: impl Into<String>,
        tool_identity: impl Into<String>,
        tool_version: impl Into<String>,
    ) -> Self {
        self.tool = Some(tool.into());
        self.tool_identity = Some(tool_identity.into());
        self.tool_version = Some(tool_version.into());
        self
    }
}

/// Installability verdict derived from materialization and signing evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BundleInstallability {
    NotMaterialized,
    Installable,
    RequiresSigning,
    NotInstallable,
}

impl BundleInstallability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotMaterialized => "not-materialized",
            Self::Installable => "installable",
            Self::RequiresSigning => "requires-signing",
            Self::NotInstallable => "not-installable",
        }
    }
}

impl BundleSigningFact {
    fn to_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            ("diagnostic".into(), optional_receipt_text(self.diagnostic.as_deref())),
            ("status".into(), text(self.status.as_str())),
            ("tool".into(), optional_receipt_text(self.tool.as_deref())),
            ("tool_identity".into(), optional_receipt_text(self.tool_identity.as_deref())),
            ("tool_version".into(), optional_receipt_text(self.tool_version.as_deref())),
        ])
        .expect("fixed bundle signing JSON keys")
    }
}

/// Machine-readable facts carried beside a packaged artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleReceipt {
    pub schema: String,
    pub plan_identity: String,
    pub kind: BundleKind,
    pub target: BundleTarget,
    pub package: String,
    pub version: String,
    pub executable_source: String,
    pub artifact_name: String,
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    pub files: Vec<BundleFileFact>,
    pub launch: LaunchDescriptor,
    pub inputs: Vec<BundleInputFact>,
    pub signing_hooks: Vec<SigningHook>,
    pub updater: Option<UpdaterManifest>,
    pub game: Option<GameBundleSpec>,
    pub materializer: Option<BundleMaterializerFact>,
    pub signing: Option<BundleSigningFact>,

}
impl BundleReceipt {
    pub fn from_plan(plan: &BundlePlan, artifact_sha256: &str) -> Self {
        Self::from_plan_with_bytes(plan, artifact_sha256, plan.logical_artifact_bytes())
    }

    pub fn from_plan_with_bytes(
        plan: &BundlePlan,
        artifact_sha256: &str,
        artifact_bytes: u64,
    ) -> Self {
        let mut files = plan
            .files
            .iter()
            .map(|file| BundleFileFact {
                path: file.path.clone(),
                kind: file.kind,
                mode: file.mode,
                bytes: file.bytes.len() as u64,
                sha256: file.digest(),
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.path.cmp(&right.path));

        Self {
            schema: PACKAGE_BUNDLE_RECEIPT_SCHEMA.into(),
            plan_identity: plan.identity.clone(),
            kind: plan.kind,
            target: plan.target,
            package: plan.package.clone(),
            version: plan.version.clone(),
            executable_source: plan.executable_source.clone(),
            artifact_name: plan.artifact_name.clone(),
            artifact_sha256: artifact_sha256.into(),
            artifact_bytes,
            files,
            launch: plan.launch.clone(),
            inputs: plan.inputs.clone(),
            signing_hooks: plan.signing_hooks.clone(),
            updater: plan.updater.clone(),
            game: plan.game.clone(),
            materializer: None,
            signing: None,
        }
}

    pub fn with_materializer(mut self, materializer: BundleMaterializerFact) -> Self {
        self.materializer = Some(materializer);
        self
    }

    pub fn with_signing(mut self, signing: BundleSigningFact) -> Self {
        self.signing = Some(signing);
        self
    }

    pub fn record_materializer(&mut self, materializer: BundleMaterializerFact) {
        self.materializer = Some(materializer);
    }

    pub fn record_signing(&mut self, signing: BundleSigningFact) {
        self.signing = Some(signing);
    }

    pub fn artifact_status(&self) -> BundleArtifactStatus {
        if self.materializer.is_some() {
            BundleArtifactStatus::Materialized
        } else {
            BundleArtifactStatus::Planned
        }
    }

    pub fn signing_status(&self) -> BundleSigningStatus {
        self.signing.as_ref().map_or_else(
            || {
                if self.signing_hooks.is_empty() {
                    BundleSigningStatus::NotRequested
                } else {
                    BundleSigningStatus::Requested
                }
            },
            |fact| fact.status,
        )
    }

    pub fn installability(&self) -> BundleInstallability {
        if self.materializer.is_none() {
            return BundleInstallability::NotMaterialized;
        }
        match self.signing_status() {
            BundleSigningStatus::Failed => BundleInstallability::NotInstallable,
            BundleSigningStatus::Requested => BundleInstallability::RequiresSigning,
            BundleSigningStatus::Signed => BundleInstallability::Installable,
            BundleSigningStatus::NotRequested if self.target == BundleTarget::WindowsMsix => {
                BundleInstallability::NotInstallable
            }
            BundleSigningStatus::NotRequested => BundleInstallability::Installable,
        }
    }

    pub fn to_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            ("artifact_bytes".into(), integer(self.artifact_bytes)),
            ("artifact_name".into(), text(&self.artifact_name)),
            ("artifact_sha256".into(), text(&self.artifact_sha256)),
            ("artifact_status".into(), text(self.artifact_status().as_str())),
            ("executable_source".into(), text(&receipt_source(&self.executable_source))),
            ("files".into(), CanonicalJson::Array(self.files.iter().map(bundle_file_receipt_json).collect())),
            ("game".into(), self.game.as_ref().map(game_spec_receipt_json).unwrap_or(CanonicalJson::Null)),
            ("inputs".into(), CanonicalJson::Array(self.inputs.iter().map(bundle_input_receipt_json).collect())),
            ("installability".into(), text(self.installability().as_str())),
            ("kind".into(), text(self.kind.as_str())),
            ("launch".into(), launch_json(&self.launch)),
            (
                "materializer".into(),
                self.materializer
                    .as_ref()
                    .map(BundleMaterializerFact::to_json)
                    .unwrap_or(CanonicalJson::Null),
            ),
            ("package".into(), text(&self.package)),
            ("plan_identity".into(), text(&self.plan_identity)),
            ("schema".into(), text(&self.schema)),
            (
                "signing".into(),
                self.signing
                    .as_ref()
                    .map(BundleSigningFact::to_json)
                    .unwrap_or(CanonicalJson::Null),
            ),
            (
                "signing_hooks".into(),
                CanonicalJson::Array(self.signing_hooks.iter().map(signing_hook_receipt_json).collect()),
            ),
            ("signing_status".into(), text(self.signing_status().as_str())),
            ("target".into(), text(self.target.as_str())),
            (
                "updater".into(),
                self.updater
                    .as_ref()
                    .map(updater_receipt_json)
                    .unwrap_or(CanonicalJson::Null),
            ),
            ("version".into(), text(&self.version)),
        ])
        .expect("fixed bundle receipt JSON keys")

    }
    pub fn canonical_bytes(&self) -> Vec<u8> {
        self.to_json().bytes()
    }

    pub fn json(&self) -> String {
        String::from_utf8(self.canonical_bytes()).expect("canonical bundle receipt JSON is UTF-8")
    }
}

/// Identity and authority facts for the adapter that produced the transport.
/// `tool_identity` is the SHA-256 digest of the resolved executable.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BundleMaterializerFact {
    pub tool: String,
    pub tool_identity: String,
    pub tool_version: String,
    pub executable: String,
    pub authority: String,
    pub mechanism: String,
    pub policy: String,
}

impl BundleMaterializerFact {
    pub fn to_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            ("authority".into(), text(&receipt_safe_text(&self.authority))),
            ("executable".into(), text(&receipt_safe_text(&self.executable))),
            ("mechanism".into(), text(&receipt_safe_text(&self.mechanism))),
            ("policy".into(), text(&receipt_safe_text(&self.policy))),
            ("tool".into(), text(&receipt_safe_text(&self.tool))),
            ("tool_identity".into(), text(&receipt_safe_text(&self.tool_identity))),
            ("tool_version".into(), text(&receipt_safe_text(&self.tool_version))),
        ])
        .expect("fixed bundle materializer JSON keys")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleFileFact {
    pub path: String,
    pub kind: BundleFileKind,
    pub mode: u32,
    pub bytes: u64,
    pub sha256: String,
}

fn bundle_file_receipt_json(file: &BundleFileFact) -> CanonicalJson {
    CanonicalJson::object([
        ("bytes".into(), integer(file.bytes)),
        ("kind".into(), text(file.kind.as_str())),
        ("mode".into(), integer(file.mode)),
        ("path".into(), text(&receipt_source(&file.path))),
        ("sha256".into(), text(&file.sha256)),
    ])
    .expect("fixed receipt bundle file fact JSON keys")
}


#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleInputFact {
    pub role: String,
    pub path: String,
    pub digest: String,
    pub bytes: u64,
}

impl BundleInputFact {
    fn to_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            ("bytes".into(), integer(self.bytes)),
            ("digest".into(), text(&self.digest)),
            ("path".into(), text(&self.path)),
            ("role".into(), text(&self.role)),
        ])
        .expect("fixed bundle input fact JSON keys")
    }
}

/// Alias used by command adapters and game packaging callers.
pub type PackageBundleSpec = BundleSpec;
pub type PackageBundlePlan = BundlePlan;
pub type BundleFilePlan = BundleFile;
pub type PackageTarget = BundleTarget;
pub type PackageKind = BundleKind;
pub type PackageBundleArtifact = BundleArtifact;
pub type PackageBundleReceipt = BundleReceipt;
pub type UpdateManifest = UpdaterManifest;
pub type SigningInput = SigningHookInput;

pub fn package_bundle_plan(spec: &BundleSpec) -> Result<BundlePlan, BundleError> {
    BundlePlan::from_spec(spec)
}

pub fn emit_package_bundle(spec: &BundleSpec) -> Result<BundleArtifact, BundleError> {
    Ok(BundlePlan::from_spec(spec)?.emit())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BundleError {
    EmptyField(&'static str),
    InvalidField { field: String, reason: String },
    InvalidPath { field: String, path: String },
    DuplicatePath(String),
    InvalidWindowsVersion(String),
}

impl fmt::Display for BundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "bundle field `{field}` is empty"),
            Self::InvalidField { field, reason } => write!(formatter, "bundle field `{field}` {reason}"),
            Self::InvalidPath { field, path } => write!(formatter, "bundle {field} path `{path}` is invalid"),
            Self::DuplicatePath(path) => write!(formatter, "bundle emits duplicate path `{path}`"),
            Self::InvalidWindowsVersion(version) => write!(formatter, "bundle version `{version}` is not a Windows MSIX version"),
        }
    }
}

impl std::error::Error for BundleError {}

fn validate_spec(spec: &BundleSpec) -> Result<(), BundleError> {
    validate_component("package", &spec.package)?;
    validate_component("version", &spec.version)?;
    validate_component("executable.name", &spec.executable.name)?;
    validate_source("executable.source", &spec.executable.source)?;
    validate_component("metadata.identifier", &spec.metadata.identifier)?;
    validate_text("metadata.display_name", &spec.metadata.display_name)?;
    validate_text("metadata.publisher", &spec.metadata.publisher)?;
    validate_text("metadata.description", &spec.metadata.description)?;
    for category in &spec.metadata.categories {
        validate_text("metadata.category", category)?;
    }
    if spec.target == BundleTarget::WindowsMsix {
        if spec.metadata.publisher.trim().is_empty() {
            return Err(BundleError::EmptyField("metadata.publisher"));
        }
        windows_version(&spec.version)?;
    }
    for icon in &spec.icons {
        validate_component("icon.name", &icon.name)?;
        if icon.size == 0 {
            return Err(BundleError::InvalidField {
                field: "icon.size".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if let Some(source) = icon.source.as_deref() {
            validate_source("icon.source", source)?;
        }
    }
    validate_target_metadata(spec)?;
    for hook in &spec.signing_hooks {
        validate_component("signing_hook.name", &hook.name)?;
        validate_source("signing_hook.program", &hook.program)?;
        for argument in &hook.args {
            validate_text("signing_hook.argument", argument)?;
        }
        for input in &hook.inputs {
            validate_source("signing_hook.input.path", &input.path)?;
            validate_component("signing_hook.input.digest", &input.digest)?;
        }
    }
    if let Some(game) = &spec.game {
        if spec.kind != BundleKind::Game {
            return Err(BundleError::InvalidField {
                field: "game".into(),
                reason: "is only valid for a game bundle".into(),
            });
        }
        validate_component("game.profile", &game.profile)?;
        validate_component("game.export_preset", &game.export_preset)?;
        validate_text("game.source_revision", &game.source_revision)?;
        validate_text("game.build_identity", &game.build_identity)?;
        validate_component("game.backend", &game.backend)?;
        validate_component("game.renderer", &game.renderer)?;
        validate_component("game.cook_mode", &game.cook_mode)?;
        validate_component("game.crash_reporter", &game.crash_reporter)?;
        validate_component("game.crash_consent", &game.crash_consent)?;
        validate_game_crash_runtime(&game.crash_reporter, &game.crash_consent, &game.crash_runtime)?;
        let mut asset_paths = BTreeSet::new();
        for asset in &game.assets {
            validate_game_asset_path(&asset.path)?;
            if !asset_paths.insert(asset.path.clone()) {
                return Err(BundleError::DuplicatePath(asset.path.clone()));
            }
        }
    } else if spec.kind == BundleKind::Game {
        return Err(BundleError::InvalidField {
            field: "game".into(),
            reason: "game bundles need game profile and asset facts".into(),
        });
    }
    if let Some(updater) = &spec.updater {
        validate_component("updater.channel", &updater.channel)?;
        validate_url("updater.url", &updater.url)?;
        if let Some(url) = updater.signature_url.as_deref() {
            validate_url("updater.signature_url", url)?;
        }
        if let Some(url) = updater.release_notes_url.as_deref() {
            validate_url("updater.release_notes_url", url)?;
        }
    }
    Ok(())
}
fn validate_target_metadata(spec: &BundleSpec) -> Result<(), BundleError> {
    match spec.target {
        BundleTarget::LinuxAppImage => {
            if spec.icons.iter().any(|icon| matches!(icon.format, IconFormat::Icns | IconFormat::Ico)) {
                return Err(BundleError::InvalidField {
                    field: "icon.format".into(),
                    reason: "is not supported by linux-appimage".into(),
                });
            }
        }
        BundleTarget::MacOSApp => {
            if spec.icons.iter().any(|icon| icon.format != IconFormat::Icns) {
                return Err(BundleError::InvalidField {
                    field: "icon.format".into(),
                    reason: "macos-app requires icns icons".into(),
                });
            }
        }
        BundleTarget::WindowsMsix => {
            if spec.metadata.identifier.len() > 50
                || !spec.metadata.identifier.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
                || !spec.metadata.identifier.as_bytes().first().is_some_and(|byte| byte.is_ascii_alphanumeric())
            {
                return Err(BundleError::InvalidField {
                    field: "metadata.identifier".into(),
                    reason: "must be an MSIX-safe identifier (ASCII letters, digits, dots, or hyphens; max 50 bytes)".into(),
                });
            }
            if !spec.metadata.publisher.trim_start().starts_with("CN=") {
                return Err(BundleError::InvalidField {
                    field: "metadata.publisher".into(),
                    reason: "must be a distinguished name beginning with `CN=` for windows-msix".into(),
                });
            }
            if spec.icons.iter().any(|icon| !matches!(icon.format, IconFormat::Png | IconFormat::Ico)) {
                return Err(BundleError::InvalidField {
                    field: "icon.format".into(),
                    reason: "windows-msix supports png or ico icons".into(),
                });
            }
        }
    }
    Ok(())
}

fn validate_game_crash_runtime(
    reporter: &str,
    consent: &str,
    runtime: &GameCrashReporterSpec,
) -> Result<(), BundleError> {
    if !matches!(reporter, "off" | "on" | "opt-in") {
        return Err(BundleError::InvalidField {
            field: "game.crash_reporter".into(),
            reason: "must be `off`, `on`, or `opt-in`".into(),
        });
    }
    if !matches!(consent, "not-requested" | "granted" | "denied") {
        return Err(BundleError::InvalidField {
            field: "game.crash_consent".into(),
            reason: "must be `not-requested`, `granted`, or `denied`".into(),
        });
    }
    if reporter == "on" && consent != "granted" {
        return Err(BundleError::InvalidField {
            field: "game.crash_consent".into(),
            reason: "must be `granted` when game crash reporter is `on`".into(),
        });
    }
    if reporter == "off" && consent == "granted" {
        return Err(BundleError::InvalidField {
            field: "game.crash_consent".into(),
            reason: "cannot be `granted` when game crash reporter is `off`".into(),
        });
    }
    if !matches!(
        runtime.installation.as_str(),
        "not-installed" | "planned" | "installed"
    ) {
        return Err(BundleError::InvalidField {
            field: "game.crash_runtime.installation".into(),
            reason: "must be `not-installed`, `planned`, or `installed`".into(),
        });
    }
    if runtime.upload_policy != "never" {
        return Err(BundleError::InvalidField {
            field: "game.crash_runtime.upload_policy".into(),
            reason: "must be `never` until an explicit upload transport is authorized".into(),
        });
    }
    if runtime.routing != "local" {
        return Err(BundleError::InvalidField {
            field: "game.crash_runtime.routing".into(),
            reason: "must be `local` until an explicit upload transport is authorized".into(),
        });
    }
    if runtime.memory_stacks != GAME_CRASH_MAX_STACKS {
        return Err(BundleError::InvalidField {
            field: "game.crash_runtime.memory_stacks".into(),
            reason: format!("must equal the canonical bound {GAME_CRASH_MAX_STACKS}"),
        });
    }
    if runtime.memory_logs != GAME_CRASH_MAX_LOGS {
        return Err(BundleError::InvalidField {
            field: "game.crash_runtime.memory_logs".into(),
            reason: format!("must equal the canonical bound {GAME_CRASH_MAX_LOGS}"),
        });
    }
    let active = reporter != "off" && consent == "granted";
    if active {
        if !matches!(runtime.installation.as_str(), "planned" | "installed") {
            return Err(BundleError::InvalidField {
                field: "game.crash_runtime.installation".into(),
                reason: "an active reporter must be planned or installed".into(),
            });
        }
        if runtime.retention_mode != "bounded-persisted"
            || runtime.retention_limit != GAME_CRASH_MAX_PERSISTED
        {
            return Err(BundleError::InvalidField {
                field: "game.crash_runtime.retention".into(),
                reason: format!(
                    "active reporter must persist exactly {GAME_CRASH_MAX_PERSISTED} latest report"
                ),
            });
        }
        validate_game_asset_path(&runtime.retention_path)?;
        if runtime.retention_path != "crash-reports" {
            return Err(BundleError::InvalidField {
                field: "game.crash_runtime.retention_path".into(),
                reason: "must use the package-local `crash-reports` directory".into(),
            });
        }
    } else if runtime.installation == "installed" {
        return Err(BundleError::InvalidField {
            field: "game.crash_runtime.installation".into(),
            reason: "a non-active reporter cannot be installed".into(),
        });
    } else if runtime.retention_mode != "none"
        || !runtime.retention_path.is_empty()
        || runtime.retention_limit != 0
    {
        return Err(BundleError::InvalidField {
            field: "game.crash_runtime.retention".into(),
            reason: "non-active reporter must not retain reports".into(),
        });
    }
    Ok(())
}

fn validate_game_asset_path(path: &str) -> Result<(), BundleError> {
    if path.is_empty() || path.contains('\\') || path.chars().any(char::is_control) {
        return Err(BundleError::InvalidPath {
            field: "game.asset".into(),
            path: path.into(),
        });
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(BundleError::InvalidPath {
                field: "game.asset".into(),
                path: path.into(),
            });
        }
    }
    Ok(())
}
fn validate_component(field: &'static str, value: &str) -> Result<(), BundleError> {
    if value.trim().is_empty() {
        return Err(BundleError::EmptyField(field));
    }
    if value.chars().any(|character| {
        character.is_control()
            || matches!(character, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
    }) {
        return Err(BundleError::InvalidField {
            field: field.into(),
            reason: "contains a path or control character".into(),
        });
    }
    Ok(())
}

fn validate_text(field: &'static str, value: &str) -> Result<(), BundleError> {
    if value.chars().any(char::is_control) {
        return Err(BundleError::InvalidField {
            field: field.into(),
            reason: "contains a control character".into(),
        });
    }
    Ok(())
}

fn validate_source(field: &'static str, value: &str) -> Result<(), BundleError> {
    if value.trim().is_empty() {
        return Err(BundleError::EmptyField(field));
    }
    if value.chars().any(char::is_control) {
        return Err(BundleError::InvalidPath {
            field: field.into(),
            path: value.into(),
        });
    }
    Ok(())
}

fn validate_url(field: &'static str, value: &str) -> Result<(), BundleError> {
    validate_source(field, value)
}

fn windows_version(version: &str) -> Result<String, BundleError> {
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 4 || parts.iter().any(|part| part.is_empty()) {
        return Err(BundleError::InvalidWindowsVersion(version.into()));
    }
    let mut normalized = Vec::with_capacity(4);
    for part in parts {
        if !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(BundleError::InvalidWindowsVersion(version.into()));
        }
        let value = part
            .parse::<u64>()
            .map_err(|_| BundleError::InvalidWindowsVersion(version.into()))?;
        if value > u16::MAX as u64 {
            return Err(BundleError::InvalidWindowsVersion(version.into()));
        }
        normalized.push(value.to_string());
    }
    while normalized.len() < 4 {
        normalized.push("0".into());
    }
    Ok(normalized.join("."))
}

fn updater_manifest(spec: &BundleSpec, updater: &UpdaterSpec, payload_sha256: &str) -> UpdaterManifest {
    UpdaterManifest {
        schema: PACKAGE_UPDATE_MANIFEST_SCHEMA.into(),
        package: spec.package.clone(),
        version: spec.version.clone(),
        target: spec.target,
        channel: updater.channel.clone(),
        artifact: format!("{}{}", spec.metadata.identifier, spec.target.artifact_suffix()),
        url: updater.url.clone(),
        signature_url: updater.signature_url.clone(),
        release_notes_url: updater.release_notes_url.clone(),
        payload_sha256: payload_sha256.into(),
        payload_bytes: spec.executable.bytes.len() as u64,
    }
}

fn bundle_metadata_json(
    spec: &BundleSpec,
    metadata: &BundleMetadata,
    executable_digest: &str,
    icons: &[BundleIcon],
    hooks: &[SigningHook],
) -> CanonicalJson {
    let icon_values = icons
        .iter()
        .map(|icon| {
            CanonicalJson::object([
                ("bytes".into(), integer(icon.bytes.len() as u64)),
                ("format".into(), text(icon.format.as_str())),
                ("name".into(), text(&icon.name)),
                ("sha256".into(), text(&icon.digest())),
                ("size".into(), integer(icon.size)),
            ])
            .expect("fixed bundle icon JSON keys")
        })
        .collect();
    CanonicalJson::object([
        ("categories".into(), CanonicalJson::Array(metadata.categories.iter().map(|value| text(value)).collect())),
        ("description".into(), text(&metadata.description)),
        ("executable".into(), text(&spec.executable.name)),
        ("executable_sha256".into(), text(executable_digest)),
        ("format".into(), text(PACKAGE_BUNDLE_FORMAT)),
        ("icons".into(), CanonicalJson::Array(icon_values)),
        ("kind".into(), text(spec.kind.as_str())),
        ("identifier".into(), text(&metadata.identifier)),
        ("name".into(), text(&metadata.display_name)),
        ("package".into(), text(&spec.package)),
        ("publisher".into(), text(&metadata.publisher)),
        (
            "signing_hooks".into(),
            CanonicalJson::Array(hooks.iter().map(signing_hook_json).collect()),
        ),
        ("target".into(), text(spec.target.as_str())),
        ("version".into(), text(&spec.version)),
    ])
    .expect("fixed bundle metadata JSON keys")
}

fn game_manifest_path(spec: &BundleSpec) -> String {
    match spec.target {
        BundleTarget::LinuxAppImage => {
            format!("usr/share/{}/jet-game.json", spec.metadata.identifier)
        }
        BundleTarget::MacOSApp => {
            format!(
                "{}.app/Contents/Resources/jet-game.json",
                spec.metadata.identifier
            )
        }
        BundleTarget::WindowsMsix => "jet/jet-game.json".into(),
    }
}

fn game_asset_path(spec: &BundleSpec, path: &str) -> String {
    match spec.target {
        BundleTarget::LinuxAppImage => {
            format!("usr/share/{}/assets/{path}", spec.metadata.identifier)
        }
        BundleTarget::MacOSApp => {
            format!(
                "{}.app/Contents/Resources/assets/{path}",
                spec.metadata.identifier
            )
        }
        BundleTarget::WindowsMsix => format!("assets/{path}"),
    }
}

fn game_spec_json(game: &GameBundleSpec) -> CanonicalJson {
    CanonicalJson::object([
        ("assets".into(), CanonicalJson::Array(game.assets.iter().map(game_asset_json).collect())),
        ("backend".into(), text(&game.backend)),
        ("build_identity".into(), text(&game.build_identity)),
        ("cook_mode".into(), text(&game.cook_mode)),
        ("crash_consent".into(), text(&game.crash_consent)),
        ("crash_reporter".into(), text(&game.crash_reporter)),
        ("crash_runtime".into(), game_crash_runtime_json(&game.crash_runtime)),
        ("development_stripping".into(), CanonicalJson::Bool(game.development_stripping)),
        ("export_preset".into(), text(&game.export_preset)),
        ("format".into(), text("jet.game.bundle")),
        ("profile".into(), text(&game.profile)),
        ("renderer".into(), text(&game.renderer)),
        ("source_revision".into(), text(&game.source_revision)),
    ])
    .expect("fixed game bundle JSON keys")
}
fn game_spec_receipt_json(game: &GameBundleSpec) -> CanonicalJson {
    CanonicalJson::object([
        (
            "assets".into(),
            CanonicalJson::Array(game.assets.iter().map(game_asset_receipt_json).collect()),
        ),
        ("backend".into(), text(&receipt_safe_text(&game.backend))),
        ("build_identity".into(), text(&receipt_safe_text(&game.build_identity))),
        ("cook_mode".into(), text(&receipt_safe_text(&game.cook_mode))),
        ("crash_consent".into(), text(&receipt_safe_text(&game.crash_consent))),
        ("crash_reporter".into(), text(&receipt_safe_text(&game.crash_reporter))),
        (
            "crash_runtime".into(),
            game_crash_runtime_receipt_json(&game.crash_runtime),
        ),
        ("development_stripping".into(), CanonicalJson::Bool(game.development_stripping)),
        ("export_preset".into(), text(&receipt_safe_text(&game.export_preset))),
        ("format".into(), text("jet.game.bundle")),
        ("profile".into(), text(&receipt_safe_text(&game.profile))),
        ("renderer".into(), text(&receipt_safe_text(&game.renderer))),
        ("source_revision".into(), text(&receipt_source(&game.source_revision))),
    ])
    .expect("fixed receipt game JSON keys")
}

fn game_crash_runtime_json(runtime: &GameCrashReporterSpec) -> CanonicalJson {
    CanonicalJson::object([
        ("abi".into(), text(GAME_CRASH_REPORTER_ABI)),
        ("installation".into(), text(&runtime.installation)),
        (
            "memory".into(),
            CanonicalJson::object([
                ("max_logs".into(), integer(runtime.memory_logs)),
                ("max_stacks".into(), integer(runtime.memory_stacks)),
            ])
            .expect("fixed game crash memory JSON keys"),
        ),
        (
            "retention".into(),
            CanonicalJson::object([
                ("max_reports".into(), integer(runtime.retention_limit)),
                ("mode".into(), text(&runtime.retention_mode)),
                ("path".into(), text(&runtime.retention_path)),
            ])
            .expect("fixed game crash retention JSON keys"),
        ),
        ("routing".into(), text(&runtime.routing)),
        ("upload_policy".into(), text(&runtime.upload_policy)),
    ])
    .expect("fixed game crash runtime JSON keys")
}

fn game_crash_runtime_receipt_json(runtime: &GameCrashReporterSpec) -> CanonicalJson {
    CanonicalJson::object([
        ("abi".into(), text(&receipt_safe_text(GAME_CRASH_REPORTER_ABI))),
        ("installation".into(), text(&receipt_safe_text(&runtime.installation))),
        (
            "memory".into(),
            CanonicalJson::object([
                ("max_logs".into(), integer(runtime.memory_logs)),
                ("max_stacks".into(), integer(runtime.memory_stacks)),
            ])
            .expect("fixed receipt game crash memory JSON keys"),
        ),
        (
            "retention".into(),
            CanonicalJson::object([
                ("max_reports".into(), integer(runtime.retention_limit)),
                ("mode".into(), text(&receipt_safe_text(&runtime.retention_mode))),
                ("path".into(), text(&receipt_source(&runtime.retention_path))),
            ])
            .expect("fixed receipt game crash retention JSON keys"),
        ),
        ("routing".into(), text(&receipt_safe_text(&runtime.routing))),
        (
            "upload_policy".into(),
            text(&receipt_safe_text(&runtime.upload_policy)),
        ),
    ])
    .expect("fixed receipt game crash runtime JSON keys")
}


fn game_manifest_json(spec: &BundleSpec, game: &GameBundleSpec) -> CanonicalJson {
    CanonicalJson::object([
        ("game".into(), game_spec_json(game)),
        ("package".into(), text(&spec.package)),
        ("target".into(), text(spec.target.as_str())),
        ("version".into(), text(&spec.version)),
    ])
    .expect("fixed game manifest JSON keys")
}

fn game_asset_json(asset: &GameBundleAsset) -> CanonicalJson {
    CanonicalJson::object([
        ("bytes".into(), integer(asset.bytes.len() as u64)),
        ("mode".into(), integer(asset.mode)),
        ("path".into(), text(&asset.path)),
        ("sha256".into(), text(&asset.digest())),
    ])
    .expect("fixed game asset JSON keys")
}
fn game_asset_receipt_json(asset: &GameBundleAsset) -> CanonicalJson {
    CanonicalJson::object([
        ("bytes".into(), integer(asset.bytes.len() as u64)),
        ("mode".into(), integer(asset.mode)),
        ("path".into(), text(&receipt_source(&asset.path))),
        ("sha256".into(), text(&asset.digest())),
    ])
    .expect("fixed receipt game asset JSON keys")
}


fn bundle_plan_json(plan: &BundlePlan) -> CanonicalJson {
    CanonicalJson::object([
        ("artifact_name".into(), text(&plan.artifact_name)),
        ("files".into(), CanonicalJson::Array(plan.files.iter().map(file_json).collect())),
        ("game".into(), plan.game.as_ref().map(game_spec_json).unwrap_or(CanonicalJson::Null)),
        ("format".into(), text(PACKAGE_BUNDLE_FORMAT)),
        ("identity".into(), text(&plan.identity)),
        ("kind".into(), text(plan.kind.as_str())),
        ("launch".into(), launch_json(&plan.launch)),
        ("metadata".into(), metadata_json(&plan.metadata)),
        ("package".into(), text(&plan.package)),
        ("executable_source".into(), text(&plan.executable_source)),
        ("inputs".into(), CanonicalJson::Array(plan.inputs.iter().map(BundleInputFact::to_json).collect())),
        (
            "signing_hooks".into(),
            CanonicalJson::Array(plan.signing_hooks.iter().map(signing_hook_json).collect()),
        ),
        ("target".into(), text(plan.target.as_str())),
        (
            "updater".into(),
            plan.updater
                .as_ref()
                .map(UpdaterManifest::to_json)
                .unwrap_or(CanonicalJson::Null),
        ),
        ("version".into(), text(&plan.version)),
        ("schema_version".into(), integer(plan.schema_version)),
    ])
    .expect("fixed bundle plan JSON keys")
}

fn metadata_json(metadata: &BundleMetadata) -> CanonicalJson {
    CanonicalJson::object([
        ("categories".into(), CanonicalJson::Array(metadata.categories.iter().map(|value| text(value)).collect())),
        ("description".into(), text(&metadata.description)),
        ("display_name".into(), text(&metadata.display_name)),
        ("identifier".into(), text(&metadata.identifier)),
        ("publisher".into(), text(&metadata.publisher)),
    ])
    .expect("fixed bundle metadata projection keys")
}

fn file_json(file: &BundleFile) -> CanonicalJson {
    CanonicalJson::object([
        ("bytes".into(), integer(file.bytes.len() as u64)),
        ("kind".into(), text(file.kind.as_str())),
        ("mode".into(), integer(file.mode)),
        ("path".into(), text(&file.path)),
        ("sha256".into(), text(&file.digest())),
    ])
    .expect("fixed bundle file JSON keys")
}

fn launch_json(launch: &LaunchDescriptor) -> CanonicalJson {
    CanonicalJson::object([
        ("arguments_passthrough".into(), CanonicalJson::Bool(launch.arguments_passthrough)),
        ("executable".into(), text(&launch.executable)),
        ("launcher".into(), text(&launch.launcher)),
    ])
    .expect("fixed launch JSON keys")
}

fn signing_hook_json(hook: &SigningHook) -> CanonicalJson {
    CanonicalJson::object([
        ("args".into(), CanonicalJson::Array(hook.args.iter().map(|value| text(value)).collect())),
        ("inputs".into(), CanonicalJson::Array(hook.inputs.iter().map(signing_input_json).collect())),
        ("name".into(), text(&hook.name)),
        ("phase".into(), text(hook.phase.as_str())),
        ("program".into(), text(&hook.program)),
    ])
    .expect("fixed signing hook JSON keys")
}
fn signing_hook_receipt_json(hook: &SigningHook) -> CanonicalJson {
    // Hook arguments may carry a password, token, or key.  The plan keeps the
    // exact declaration for the adapter, while the receipt intentionally
    // retains only the argument count and redaction markers.
    let args = hook.args.iter().map(|_| text("<redacted>")).collect();
    CanonicalJson::object([
        ("args".into(), CanonicalJson::Array(args)),
        ("args_redacted".into(), CanonicalJson::Bool(true)),
        (
            "inputs".into(),
            CanonicalJson::Array(hook.inputs.iter().map(signing_input_receipt_json).collect()),
        ),
        ("name".into(), text(&hook.name)),
        ("phase".into(), text(hook.phase.as_str())),
        ("program".into(), text(&receipt_source(&hook.program))),
    ])
    .expect("fixed receipt signing hook JSON keys")
}

fn updater_receipt_json(updater: &UpdaterManifest) -> CanonicalJson {
    CanonicalJson::object([
        ("artifact".into(), text(&updater.artifact)),
        ("channel".into(), text(&updater.channel)),
        ("package".into(), text(&updater.package)),
        ("payload_bytes".into(), integer(updater.payload_bytes)),
        ("payload_sha256".into(), text(&updater.payload_sha256)),
        (
            "release_notes_url".into(),
            optional_receipt_url(updater.release_notes_url.as_deref()),
        ),
        ("schema".into(), text(&updater.schema)),
        ("signature_url".into(), optional_receipt_url(updater.signature_url.as_deref())),
        ("target".into(), text(updater.target.as_str())),
        ("url".into(), text(&receipt_url(&updater.url))),
        ("version".into(), text(&updater.version)),
    ])
    .expect("fixed receipt updater JSON keys")
}

fn optional_receipt_url(value: Option<&str>) -> CanonicalJson {
    value
        .map(|value| text(&receipt_url(value)))
        .unwrap_or(CanonicalJson::Null)
}

fn receipt_url(value: &str) -> String {
    if value.contains('@') || value.contains('?') || value.contains('#') {
        "<redacted>".into()
    } else {
        receipt_safe_text(value)
    }
}

fn optional_receipt_text(value: Option<&str>) -> CanonicalJson {
    value
        .map(|value| text(&receipt_safe_text(value)))
        .unwrap_or(CanonicalJson::Null)
}

fn receipt_safe_text(value: &str) -> String {
    if contains_sensitive_marker(value) {
        "<redacted>".into()
    } else {
        value.into()
    }
}

fn receipt_source(value: &str) -> String {
    if value.contains('@') || value.contains('?') || value.contains('#') {
        "<redacted>".into()
    } else {
        receipt_safe_text(value)
    }
}

fn contains_sensitive_marker(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "password",
        "passphrase",
        "secret",
        "credential",
        "api-key",
        "apikey",
        "authorization",
        "bearer ",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn signing_input_receipt_json(input: &SigningHookInput) -> CanonicalJson {
    CanonicalJson::object([
        ("digest".into(), text(&input.digest)),
        ("path".into(), text(&receipt_source(&input.path))),
    ])
    .expect("fixed receipt signing input JSON keys")
}
fn signing_input_json(input: &SigningHookInput) -> CanonicalJson {
    CanonicalJson::object([
        ("digest".into(), text(&input.digest)),
        ("path".into(), text(&input.path)),
    ])
    .expect("fixed signing input JSON keys")
}
fn bundle_input_receipt_json(input: &BundleInputFact) -> CanonicalJson {
    CanonicalJson::object([
        ("bytes".into(), integer(input.bytes)),
        ("digest".into(), text(&input.digest)),
        ("path".into(), text(&receipt_source(&input.path))),
        ("role".into(), text(&input.role)),
    ])
    .expect("fixed receipt bundle input JSON keys")
}

fn artifact_digest(files: &[BundleFile]) -> String {
    let mut ordered = files.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.path.cmp(&right.path));
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"jet-package-artifact-v1\0");
    for file in ordered {
        frame(&mut bytes, file.path.as_bytes());
        frame(&mut bytes, file.kind.as_str().as_bytes());
        bytes.extend_from_slice(&file.mode.to_be_bytes());
        frame(&mut bytes, &file.bytes);
    }
    SHA256::sha256_hex(&bytes)
}

fn frame(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

fn text(value: &str) -> CanonicalJson {
    CanonicalJson::String(value.into())
}

fn optional_text(value: Option<&str>) -> CanonicalJson {
    value.map(text).unwrap_or(CanonicalJson::Null)
}

fn integer(value: impl ToString) -> CanonicalJson {
    CanonicalJson::Integer(value.to_string())
}

fn linux_launcher(executable: &str) -> Vec<u8> {
    let executable = shell_double_quote(executable);
    format!(
        "#!/bin/sh\nset -eu\nHERE=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nexec \"$HERE/usr/bin/{executable}\" \"$@\"\n"
    )
    .into_bytes()
}

fn shell_double_quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

fn linux_desktop_file(metadata: &BundleMetadata, icon_name: Option<&str>) -> Vec<u8> {
    let mut out = String::from("[Desktop Entry]\nType=Application\nVersion=1.0\n");
    let _ = writeln!(out, "Name={}", desktop_escape(&metadata.display_name));
    let _ = writeln!(out, "Exec=AppRun");
    if let Some(icon) = icon_name {
        let _ = writeln!(out, "Icon={}", desktop_escape(icon));
    }
    if !metadata.description.is_empty() {
        let _ = writeln!(out, "Comment={}", desktop_escape(&metadata.description));
    }
    out.push_str("Terminal=false\n");
    if !metadata.categories.is_empty() {
        let _ = writeln!(out, "Categories={};", metadata.categories.join(";"));
    }
    out.into_bytes()
}

fn desktop_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n")
}

fn mac_info_plist(spec: &BundleSpec, icon_name: Option<&str>) -> Vec<u8> {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n",
    );
    let _ = writeln!(out, "<key>CFBundleDisplayName</key><string>{}</string>", xml_escape(&spec.metadata.display_name));
    let _ = writeln!(out, "<key>CFBundleExecutable</key><string>{}</string>", xml_escape(&spec.executable.name));
    let _ = writeln!(out, "<key>CFBundleIdentifier</key><string>{}</string>", xml_escape(&spec.metadata.identifier));
    let _ = writeln!(out, "<key>CFBundleName</key><string>{}</string>", xml_escape(&spec.metadata.display_name));
    let _ = writeln!(out, "<key>CFBundleShortVersionString</key><string>{}</string>", xml_escape(&spec.version));
    let _ = writeln!(out, "<key>CFBundleVersion</key><string>{}</string>", xml_escape(&spec.version));
    if let Some(icon) = icon_name {
        let _ = writeln!(out, "<key>CFBundleIconFile</key><string>{}</string>", xml_escape(icon));
    }
    out.push_str("</dict></plist>\n");
    out.into_bytes()
}

fn windows_executable_name(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".exe") {
        name.to_string()
    } else {
        format!("{name}.exe")
    }
}

fn windows_manifest(
    spec: &BundleSpec,
    executable: &str,
    logo: Option<&str>,
) -> Result<Vec<u8>, BundleError> {
    let version = windows_version(&spec.version)?;
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Package xmlns=\"http://schemas.microsoft.com/appx/manifest/foundation/windows10\" xmlns:uap=\"http://schemas.microsoft.com/appx/manifest/uap/windows10\" IgnorableNamespaces=\"uap\">\n",
    );
    let _ = writeln!(
        out,
        "<Identity Name=\"{}\" Publisher=\"{}\" Version=\"{}\" />",
        xml_escape(&spec.metadata.identifier),
        xml_escape(&spec.metadata.publisher),
        version
    );
    out.push_str("<Properties>\n");
    let _ = writeln!(out, "<DisplayName>{}</DisplayName>", xml_escape(&spec.metadata.display_name));
    let _ = writeln!(out, "<PublisherDisplayName>{}</PublisherDisplayName>", xml_escape(&spec.metadata.publisher));
    if let Some(logo) = logo {
        let _ = writeln!(out, "<Logo>{}</Logo>", xml_escape(logo));
    }
    out.push_str("</Properties>\n<Applications>\n");
    let _ = writeln!(
        out,
        "<Application Id=\"App\" Executable=\"{}\" EntryPoint=\"Windows.FullTrustApplication\">",
        xml_escape(executable)
    );
    let _ = write!(
        out,
        "<uap:VisualElements AppListEntry=\"none\" DisplayName=\"{}\"",
        xml_escape(&spec.metadata.display_name)
    );
    if let Some(logo) = logo {
        let _ = write!(
            out,
            " Square44x44Logo=\"{}\" Square150x150Logo=\"{}\"",
            xml_escape(logo),
            xml_escape(logo)
        );
    }
    out.push_str(" />\n</Application>\n</Applications>\n</Package>\n");
    Ok(out.into_bytes())
}

fn windows_content_types(files: &[BundleFile]) -> Vec<u8> {
    let mut extensions = BTreeSet::new();
    for file in files {
        let extension = file
            .path
            .rsplit('/')
            .next()
            .and_then(|name| name.rsplit_once('.').map(|(_, extension)| extension))
            .unwrap_or("");
        if !extension.is_empty() {
            extensions.insert(extension.to_ascii_lowercase());
        }
    }
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
    );
    for extension in extensions {
        let content_type = match extension.as_str() {
            "xml" => "application/xml",
            "json" => "application/json",
            "exe" => "application/octet-stream",
            "png" => "image/png",
            "ico" => "image/x-icon",
            "icns" => "image/icns",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        };
        let _ = write!(
            out,
            "<Default Extension=\"{}\" ContentType=\"{}\"/>",
            xml_escape(&extension),
            content_type
        );
    }
    out.push_str("</Types>\n");
    out.into_bytes()
}

fn sha256_base64(hex: &str) -> String {
    let bytes = hex.as_bytes();
    if bytes.len() % 2 != 0 {
        return hex.to_owned();
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let Some(high) = hex_nibble(pair[0]) else {
            return hex.to_owned();
        };
        let Some(low) = hex_nibble(pair[1]) else {
            return hex.to_owned();
        };
        decoded.push((high << 4) | low);
    }
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity((decoded.len() + 2) / 3 * 4);
    for chunk in decoded.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        encoded.push(ALPHABET[((value >> 18) & 0x3f) as usize] as char);
        encoded.push(ALPHABET[((value >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(ALPHABET[((value >> 6) & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(ALPHABET[(value & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn windows_block_map(files: &[BundleFile]) -> Vec<u8> {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<BlockMap xmlns=\"http://schemas.microsoft.com/appx/2010/blockmap\" HashMethod=\"http://www.w3.org/2001/04/xmlenc#sha256\">",
    );
    for file in files {
        let _ = write!(
            out,
            "<File Name=\"{}\" Size=\"{}\"><Block Hash=\"{}\" /></File>",
            xml_escape(&file.path),
            file.bytes.len(),
            sha256_base64(&file.digest())
        );
    }
    out.push_str("</BlockMap>\n");
    out.into_bytes()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
