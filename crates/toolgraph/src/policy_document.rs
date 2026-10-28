// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Complete offline policy documents from caller-prepared typed declarations.
use std::fmt;

use purrassist_core::{
    Governor, ScratchCharge, ScratchString, ScratchVec, StopReason,
    contract::{Resource, saturating_size as size},
    digest::Digest,
};

use crate::apparmor_glob::emit;
use crate::{
    AppArmorGlob, CapabilityRule, FileExecution, FilePermissions, FileRule, FileRuleError,
    FileTarget, GlobError, InternetError, InternetLimits, InternetRule, IpcError,
    LiteralPathBinding, MountRule, PrivilegeError, PtraceAccessSet, PtraceRule, ResourceLimitError,
    ResourceLimits, SignalRule, UnixError, UnixRule, dangerous_capability_denials,
    literal_path_word,
};

/// Explicit offline ABI selection; this is not running-kernel attestation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyTarget {
    /// Basic ABI 3.0; fine-grained Internet declarations are refused.
    Abi3,
    /// ABI 4.0-ip, including the existing scoped Internet rule family.
    Abi4Ip,
}
impl PolicyTarget {
    /// The exact ABI name emitted by the document compiler.
    ///
    /// ```
    /// use purrassist_toolgraph::PolicyTarget;
    /// assert_eq!(PolicyTarget::Abi4Ip.abi(), "4.0-ip");
    /// ```
    #[must_use]
    pub const fn abi(self) -> &'static str {
        match self {
            Self::Abi3 => "3.0",
            Self::Abi4Ip => "4.0-ip",
        }
    }
}

/// One original typed declaration. Rendering always uses its existing authority.
#[derive(Clone, Copy, Debug)]
pub enum PolicyRule<'a> {
    /// File permissions and execution transition.
    File(FileRule<'a>),
    /// Scoped Internet access with explicit expansion ceilings.
    Internet {
        /// Original rule.
        rule: InternetRule<'a>,
        /// Original enumeration ceilings.
        limits: InternetLimits,
    },
    /// Unix address and peer mediation.
    Unix(UnixRule<'a>),
    /// Signal mediation.
    Signal(SignalRule<'a>),
    /// Ptrace mediation.
    Ptrace(PtraceRule<'a>),
    /// One named capability.
    Capability(CapabilityRule<'a>),
    /// One typed mount-operation denial.
    Mount(MountRule<'a>),
    /// All five resource ceilings, with original caller identity.
    Limits {
        /// Original identity.
        source: &'a str,
        /// Exact original values.
        limits: ResourceLimits,
    },
}
impl<'a> PolicyRule<'a> {
    const fn source(self) -> &'a str {
        match self {
            Self::File(v) => v.source,
            Self::Internet { rule, .. } => rule.source,
            Self::Unix(v) => v.source,
            Self::Signal(v) => v.source,
            Self::Ptrace(v) => v.source,
            Self::Capability(v) => v.source,
            Self::Mount(v) => v.source,
            Self::Limits { source, .. } => source,
        }
    }
    fn render(self, g: &mut Governor) -> Result<Rendered, PolicyCause<'a>> {
        let (text, abi, mediation) = match self {
            Self::File(v) => {
                let v = v.render(g).map_err(PolicyCause::File)?;
                (v.text, v.requirement.abi, v.requirement.mediation)
            }
            Self::Internet { rule, limits } => {
                let v = rule.render(limits, g).map_err(PolicyCause::Internet)?;
                (v.text, v.requirement.abi, v.requirement.mediation)
            }
            Self::Unix(v) => {
                let v = v.render(g).map_err(PolicyCause::Unix)?;
                (v.text, v.requirement.abi, v.requirement.mediation)
            }
            Self::Signal(v) => {
                let v = v.render(g).map_err(PolicyCause::Ipc)?;
                (v.text, v.requirement.abi, v.requirement.mediation)
            }
            Self::Ptrace(v) => {
                let v = v.render(g).map_err(PolicyCause::Ipc)?;
                (v.text, v.requirement.abi, v.requirement.mediation)
            }
            Self::Capability(v) => {
                let v = v.render(g).map_err(PolicyCause::Privilege)?;
                (v.text, v.requirement.abi, v.requirement.mediation)
            }
            Self::Mount(v) => {
                let v = v.render(g).map_err(PolicyCause::Privilege)?;
                (v.text, v.requirement.abi, v.requirement.mediation)
            }
            Self::Limits { source, limits } => {
                let v = limits.render(source, g).map_err(PolicyCause::Limits)?;
                (v.text, v.requirement.abi, v.requirement.mediation)
            }
        };
        Ok(Rendered {
            text,
            abi,
            mediation,
        })
    }
}
struct Rendered {
    text: String,
    abi: &'static str,
    mediation: &'static str,
}

/// One mandatory include occurrence, in the exact caller-prepared order.
/// The path names a declared dependency; it is never resolved from the filesystem.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{PolicyImport, PolicyDependency, PolicyBody, PolicyProfile,
///     PolicyInput, PolicyTarget, PolicyDocument, PolicyOrigin};
/// let dependencies = [PolicyDependency { source: "file:example",
///     path: "abstractions/example", bytes: b"/srv/example r,\n" }];
/// let imports = [PolicyImport { source: "include:example", path: dependencies[0].path }];
/// let profiles = [PolicyProfile::Standalone(PolicyBody { source: "body", name: "worker",
///     rules: &[], imports: &imports })];
/// let mut g = Governor::new(Budget::default());
/// let document = PolicyDocument::prepare(PolicyInput { source: "document",
///     target: PolicyTarget::Abi3, home: "/home/worker", profiles: &profiles,
///     dependencies: &dependencies }, &mut g).unwrap();
/// let output = document.compile(&mut g).unwrap();
/// assert!(output.text.contains("include <abstractions/example>"));
/// let unknown = output.requirements.complete_mediation().err().unwrap();
/// assert!(matches!(output.spans[unknown.first_import_span].origin, PolicyOrigin::Import { .. }));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct PolicyImport<'a> {
    /// Original occurrence identity, unique among imports in this body.
    pub source: &'a str,
    /// Normalized `abstractions/...` relative file path.
    pub path: &'a str,
}

/// One opaque caller-provided dependency file. Contents are hashed, never parsed
/// as permissions or interpolated into the generated profile body.
#[derive(Clone, Copy, Debug)]
pub struct PolicyDependency<'a> {
    /// Original file identity.
    pub source: &'a str,
    /// Normalized `abstractions/...` relative file path.
    pub path: &'a str,
    /// Exact external file bytes; further imports remain opaque.
    pub bytes: &'a [u8],
}

/// Byte identity of one declared external file, including transitive records.
#[derive(Clone, Copy, Debug)]
pub struct BoundPolicyDependency<'a> {
    /// Unchanged original dependency record.
    pub dependency: PolicyDependency<'a>,
    /// SHA-256 of the exact supplied bytes, without filesystem attestation.
    pub digest: Digest,
}
impl ScratchCharge for BoundPolicyDependency<'_> {
    const CHARGE: usize = 80;
}
const _: () = assert!(core::mem::size_of::<BoundPolicyDependency<'_>>() <= 80);

/// One named body. Names are validated through the existing literal [`FileTarget`] API.
#[derive(Clone, Copy, Debug)]
pub struct PolicyBody<'a> {
    /// Original source identity, retained as data and never interpolated.
    pub source: &'a str,
    /// Exact standalone, parent or local hat name, without a leading caret.
    pub name: &'a str,
    /// Typed caller rules; identities must be unique within this body.
    pub rules: &'a [PolicyRule<'a>],
    /// Mandatory import occurrences, emitted in caller order without deduplication.
    pub imports: &'a [PolicyImport<'a>],
}
/// A complete standalone profile or a parent with flat hats.
#[derive(Clone, Copy, Debug)]
pub enum PolicyProfile<'a> {
    /// Standalone executable profile.
    Standalone(PolicyBody<'a>),
    /// Parent profile and its local hats; no nested hats or raw child syntax.
    Parent {
        /// Original parent body.
        body: PolicyBody<'a>,
        /// Original local hats.
        hats: &'a [PolicyBody<'a>],
    },
}
impl<'a> PolicyProfile<'a> {
    const fn body(self) -> PolicyBody<'a> {
        match self {
            Self::Standalone(body) | Self::Parent { body, .. } => body,
        }
    }
}
/// Original complete caller input, without graph admission or dispatch authority.
#[derive(Clone, Copy, Debug)]
pub struct PolicyInput<'a> {
    /// Original document identity, never emitted as policy syntax.
    pub source: &'a str,
    /// Explicit selected offline target.
    pub target: PolicyTarget,
    /// Absolute literal HOME directory. No environment or filesystem lookup occurs.
    pub home: &'a str,
    /// At least one standalone or parent profile.
    pub profiles: &'a [PolicyProfile<'a>],
    /// Opaque external files, including caller-prepared transitive dependencies.
    pub dependencies: &'a [PolicyDependency<'a>],
}

/// Fixed compiler additions, distinguished from every caller-authored identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyBaseline {
    /// SSH file denial under the exact caller HOME.
    Ssh,
    /// Environment-file denial.
    Environment,
    /// The existing complete dangerous-capability deny set.
    Capabilities,
    /// Broad ptrace denial.
    Ptrace,
    /// Mount denial.
    Mount,
    /// Remount denial.
    Remount,
    /// Unmount denial.
    Umount,
    /// Pivot-root denial.
    PivotRoot,
}
/// Original caller rule or an explicit compiler addition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyOrigin<'a> {
    /// Unchanged caller identity.
    Caller(&'a str),
    /// One import occurrence and its exact bound dependency manifest index.
    Import {
        /// Original occurrence identity.
        source: &'a str,
        /// Index in [`CompiledPolicy::dependencies`].
        dependency: usize,
    },
    /// A fixed addition; never presented as an original caller rule.
    Baseline(PolicyBaseline),
}
/// Exact output span of one rule fragment, including its indentation/newlines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicySpan<'a> {
    /// Complete parent/standalone profile name.
    pub profile: &'a str,
    /// Local hat name where applicable.
    pub hat: Option<&'a str>,
    /// Original rule identity or explicit addition kind.
    pub origin: PolicyOrigin<'a>,
    /// First byte in the complete document.
    pub start: usize,
    /// Exclusive final byte in the complete document.
    pub end: usize,
}
impl ScratchCharge for PolicySpan<'_> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 80);
        80
    };
}

const MEDIATION: [&str; 8] = [
    "file",
    "caps",
    "network_v8/af_inet",
    "network/af_unix",
    "signal",
    "ptrace",
    "mount",
    "rlimit",
];
/// Local offline classes with an explicit opaque-import completeness barrier.
/// No kernel or loaded-policy evidence is implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyRequirements {
    target: PolicyTarget,
    classes: u8,
    first_opaque_import: Option<usize>,
}
impl PolicyRequirements {
    /// Selected offline target, as supplied originally. See [`PolicyTarget`]'s example.
    #[must_use]
    pub const fn target(self) -> PolicyTarget {
        self.target
    }
    /// Classes required by local typed rules only, excluding opaque dependencies.
    /// These are not a complete requirement union when imports exist.
    pub fn local_mediation(self) -> impl Iterator<Item = &'static str> {
        MEDIATION
            .into_iter()
            .enumerate()
            .filter_map(move |(i, name)| (self.classes & (1 << i) != 0).then_some(name))
    }
    /// Complete known classes, refusing to hide opaque imported requirements.
    /// This offline result never establishes dispatch authority or kernel support.
    ///
    /// # Errors
    /// Returns [`UnresolvedPolicyImports`] whenever an opaque import is emitted.
    /// The [`PolicyDocument`] example covers the no-import success case.
    pub fn complete_mediation(
        self,
    ) -> Result<impl Iterator<Item = &'static str>, UnresolvedPolicyImports> {
        if let Some(first_import_span) = self.first_opaque_import {
            return Err(UnresolvedPolicyImports { first_import_span });
        }
        Ok(self.local_mediation())
    }
    fn add<'a>(
        &mut self,
        source: &'a str,
        abi: &'static str,
        mediation: &'static str,
        g: &mut Governor,
    ) -> Result<(), PolicyCause<'a>> {
        // Eight fixed class names have at most 19 bytes; two ABI literals
        // have at most six. This covers comparisons plus fixed bitset work.
        g.charge(Resource::Work, 256)?;
        if abi == "4.0-ip" && self.target == PolicyTarget::Abi3 {
            return Err(PolicyCause::Target {
                source,
                target: self.target,
                required_abi: abi,
                mediation,
            });
        }
        if !matches!(abi, "3.0" | "4.0-ip") {
            return Err(PolicyCause::Requirement { abi, mediation });
        }
        let Some(index) = MEDIATION.iter().position(|name| *name == mediation) else {
            return Err(PolicyCause::Requirement { abi, mediation });
        };
        self.classes |= 1 << index;
        Ok(())
    }
}

/// Opaque imports prevent any complete feature-requirement claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnresolvedPolicyImports {
    /// First opaque import's index in [`CompiledPolicy::spans`].
    pub first_import_span: usize,
}
impl fmt::Display for UnresolvedPolicyImports {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "opaque import at source span {} requires external dependency and feature analysis",
            self.first_import_span
        )
    }
}
impl std::error::Error for UnresolvedPolicyImports {}

/// Typed original native refusal or document-structure error.
#[derive(Debug)]
pub enum PolicyCause<'a> {
    /// No profiles were supplied.
    Empty,
    /// An import or dependency path is not an admitted literal relative file.
    ImportPath {
        /// Original occurrence or dependency identity.
        source: &'a str,
        /// Unchanged original path.
        path: &'a str,
        /// First refused original byte offset.
        offset: usize,
    },
    /// A mandatory direct import has no declared dependency record.
    MissingDependency {
        /// Original import request.
        import: PolicyImport<'a>,
    },
    /// Two declared files claim the same path; neither record was selected.
    DependencyConflict {
        /// First exact original record.
        first: &'a PolicyDependency<'a>,
        /// Conflicting exact original record, even when its bytes are identical.
        second: &'a PolicyDependency<'a>,
    },
    /// HOME is not an absolute literal directory.
    Home {
        /// Original input.
        home: &'a str,
    },
    /// A root name, local hat name or caller identity was repeated in its scope.
    Duplicate {
        /// Original repeated bytes.
        name: &'a str,
        /// Scope of the duplicate.
        scope: PolicyDuplicateScope,
    },
    /// Exact existing file/name failure.
    File(FileRuleError<'a>),
    /// Exact existing Internet failure.
    Internet(InternetError<'a>),
    /// Exact existing Unix failure.
    Unix(UnixError<'a>),
    /// Exact existing IPC failure.
    Ipc(IpcError<'a>),
    /// Exact existing capability/mount failure.
    Privilege(PrivilegeError<'a>),
    /// Exact existing resource-limit failure.
    Limits(ResourceLimitError<'a>),
    /// Exact baseline glob/literal preparation failure.
    Glob(GlobError),
    /// The selected document target cannot support this rule.
    Target {
        /// Original caller identity.
        source: &'a str,
        /// Original selected target.
        target: PolicyTarget,
        /// Rule's required offline ABI.
        required_abi: &'static str,
        /// Rule's required mediation.
        mediation: &'static str,
    },
    /// An existing renderer returned an unknown internal requirement.
    Requirement {
        /// Unexpected ABI.
        abi: &'static str,
        /// Unexpected native class.
        mediation: &'static str,
    },
    /// Exact new-collection/output/cancellation failure.
    Resource(StopReason),
}
impl From<StopReason> for PolicyCause<'_> {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}
impl fmt::Display for PolicyCause<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "policy document refused: {self:?}")
    }
}
impl std::error::Error for PolicyCause<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File(e) => e.source(),
            Self::Internet(e) => e.source(),
            Self::Unix(e) => e.source(),
            Self::Ipc(e) => e.source(),
            Self::Privilege(e) => e.source(),
            Self::Limits(e) => e.source(),
            Self::Glob(e) => Some(e),
            Self::Resource(e) => Some(e),
            _ => None,
        }
    }
}
/// Scope whose identities must be unique.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyDuplicateScope {
    /// Top-level profile names, including parents and standalone profiles.
    Profiles,
    /// Local hats inside one parent.
    Hats,
    /// Caller rule identities inside one body; identical rules with distinct ids survive.
    Rules,
    /// Import occurrence identities inside one body; order remains unchanged.
    Imports,
}
/// Preparation refusal retaining the complete original caller input.
#[derive(Debug)]
pub struct PolicyPrepareError<'a> {
    /// Unchanged caller input.
    pub input: PolicyInput<'a>,
    /// Original typed cause.
    pub cause: PolicyCause<'a>,
}
impl fmt::Display for PolicyPrepareError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.cause.fmt(f)
    }
}
impl std::error::Error for PolicyPrepareError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.source()
    }
}
/// Compilation refusal retaining the caller's prepared document and original input.
#[derive(Debug)]
pub struct PolicyError<'s, 'a> {
    /// Original prepared document; its input remains available unchanged.
    pub document: &'s PolicyDocument<'a>,
    /// Exact native or document-level cause.
    pub cause: PolicyCause<'s>,
}
impl fmt::Display for PolicyError<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.cause.fmt(f)
    }
}
impl std::error::Error for PolicyError<'_, '_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.source()
    }
}

/// Caller-owned baseline preparation, without security admission.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{PolicyBody, PolicyDocument, PolicyInput, PolicyProfile, PolicyTarget};
/// let profiles = [PolicyProfile::Standalone(PolicyBody { source: "worker", name: "purrassist.worker", rules: &[], imports: &[] })];
/// let mut g = Governor::new(Budget::default());
/// let input = PolicyInput { source: "caller:policy", target: PolicyTarget::Abi3, home: "/home/worker", profiles: &profiles, dependencies: &[] };
/// let prepared = PolicyDocument::prepare(input, &mut g).unwrap();
/// let compiled = prepared.compile(&mut g).unwrap();
/// assert!(compiled.text.starts_with("abi <abi/3.0>,\n"));
/// assert!(compiled.requirements.complete_mediation().unwrap().any(|name| name == "ptrace"));
/// ```
#[derive(Debug)]
pub struct PolicyDocument<'a> {
    input: PolicyInput<'a>,
    home_binding: [LiteralPathBinding<'a>; 1],
    ssh: AppArmorGlob,
    environment: AppArmorGlob,
    peers: AppArmorGlob,
    files: FilePermissions,
    ptrace: PtraceAccessSet,
}
/// Complete atomic output and its original input/source mappings.
#[derive(Debug)]
pub struct CompiledPolicy<'s, 'a> {
    /// Original caller-prepared document.
    pub document: &'s PolicyDocument<'a>,
    /// Complete native policy text, never a successful partial document.
    pub text: ScratchString,
    /// Original rule/addition spans in emitted order.
    pub spans: ScratchVec<PolicySpan<'s>>,
    /// Required offline target/classes, without host attestation.
    pub requirements: PolicyRequirements,
    /// All original external files and computed byte identities, in declared order.
    /// Native resolution/transitive completeness and feature support remain unproven.
    pub dependencies: ScratchVec<BoundPolicyDependency<'s>>,
}

impl<'a> PolicyDocument<'a> {
    /// Prepare fixed baseline globs/accesses against the explicit HOME input.
    ///
    /// No environment lookup or security admission occurs. Root and trailing
    /// slash inputs append no extra separator, preserving literal path semantics.
    /// Existing detached String/glob reservations remain conservatively retained.
    ///
    /// # Errors
    /// Returns the original input and exact glob, permission/access or native
    /// resource cause. HOME must be absolute and safely representable.
    /// See the type example for a complete baseline-only policy.
    #[expect(
        clippy::result_large_err,
        reason = "Refusals retain the exact original borrowed input and native cause without ungoverned boxing."
    )]
    pub fn prepare(
        input: PolicyInput<'a>,
        g: &mut Governor,
    ) -> Result<Self, PolicyPrepareError<'a>> {
        Self::prepare_inner(input, g).map_err(|cause| PolicyPrepareError { input, cause })
    }
    fn prepare_inner(input: PolicyInput<'a>, g: &mut Governor) -> Result<Self, PolicyCause<'a>> {
        admit_identity(input.source, g)?;
        admit_identity(input.home, g)?;
        if !input.home.starts_with('/') {
            return Err(PolicyCause::Home { home: input.home });
        }
        literal_path_word(input.home, g).map_err(PolicyCause::Glob)?;
        let recipe = if input.home.ends_with('/') {
            "@{HOME}.ssh/**"
        } else {
            "@{HOME}/.ssh/**"
        };
        Ok(Self {
            input,
            home_binding: [LiteralPathBinding {
                name: "HOME",
                path: input.home,
            }],
            ssh: AppArmorGlob::parse(recipe, &["HOME"], g).map_err(PolicyCause::Glob)?,
            environment: AppArmorGlob::parse("/**/.env*", &[], g).map_err(PolicyCause::Glob)?,
            peers: AppArmorGlob::parse("**", &[], g).map_err(PolicyCause::Glob)?,
            files: FilePermissions::parse("rw", g).map_err(PolicyCause::File)?,
            ptrace: PtraceAccessSet::parse("trace,read,readby,tracedby", g)
                .map_err(PolicyCause::Ipc)?,
        })
    }
    /// Borrow the exact original input. See the type example for preparation.
    #[must_use]
    pub const fn input(&self) -> PolicyInput<'a> {
        self.input
    }

    /// Compile complete named profiles/hats through the existing rule renderers.
    ///
    /// New sorted indices, output and source maps are governed. Existing `String`-returning
    /// renderer reservations stay conservatively retained, including on later refusal;
    /// this operation does not claim prompt release of those detached reservations.
    /// Names use [`FileTarget`]; roots/hats/rules sort by their original names/ids.
    /// Imports retain caller order and bind to hashed opaque dependency records.
    /// No text is published until all bodies and final cancellation checks succeed.
    /// Complete new output/source-map storage releases when the artifact drops.
    ///
    /// # Errors
    /// Preserves original input and native rule errors; refuses empty documents,
    /// duplicate scoped identities, invalid names, incompatible selected ABI,
    /// invalid import/dependency paths, missing direct bindings, duplicate
    /// dependency-path conflicts, governance/cancellation failures and unknown
    /// internal target requirements. Opaque imports remain unresolved complete
    /// requirements even when source generation succeeds; use
    /// [`PolicyRequirements::complete_mediation`] to query that boundary.
    /// This does not establish graph admission, target existence or confinement.
    pub fn compile<'s>(
        &'s self,
        g: &mut Governor,
    ) -> Result<CompiledPolicy<'s, 'a>, PolicyError<'s, 'a>> {
        self.compile_inner(g).map_err(|cause| PolicyError {
            document: self,
            cause,
        })
    }
    fn compile_inner<'s>(
        &'s self,
        g: &mut Governor,
    ) -> Result<CompiledPolicy<'s, 'a>, PolicyCause<'s>> {
        admit_identity(self.input.source, g)?;
        if self.input.profiles.is_empty() {
            return Err(PolicyCause::Empty);
        }
        let roots = ordered(
            self.input.profiles.len(),
            |i| self.input.profiles[i].body().name,
            PolicyDuplicateScope::Profiles,
            g,
        )?;
        let dependencies = bind_dependencies(self.input.dependencies, g)?;
        let mut compiler = Compiler {
            document: self,
            dependencies,
            output: ScratchString::new(g),
            spans: ScratchVec::new(g),
            requirements: PolicyRequirements {
                target: self.input.target,
                classes: 0,
                first_opaque_import: None,
            },
        };
        emit(&mut compiler.output, "abi <abi/", g)?;
        emit(&mut compiler.output, self.input.target.abi(), g)?;
        emit(&mut compiler.output, ">,\n", g)?;
        for &index in &roots {
            let profile = self.input.profiles[index];
            let body = profile.body();
            g.charge(Resource::Work, 1)?;
            FileTarget::parse(body.name, g).map_err(PolicyCause::File)?;
            emit(&mut compiler.output, "profile ", g)?;
            emit(&mut compiler.output, body.name, g)?;
            emit(&mut compiler.output, " flags=(attach_disconnected) {\n", g)?;
            compiler.body(body, body.name, None, "  ", g)?;
            if let PolicyProfile::Parent { hats, .. } = profile {
                let sorted = ordered(hats.len(), |i| hats[i].name, PolicyDuplicateScope::Hats, g)?;
                for &hat in &sorted {
                    g.charge(Resource::Work, 1)?;
                    let hat = hats[hat];
                    FileTarget::parse(hat.name, g).map_err(PolicyCause::File)?;
                    emit(&mut compiler.output, "  ^", g)?;
                    emit(&mut compiler.output, hat.name, g)?;
                    emit(&mut compiler.output, " {\n", g)?;
                    compiler.body(hat, body.name, Some(hat.name), "    ", g)?;
                    emit(&mut compiler.output, "  }\n", g)?;
                }
            }
            emit(&mut compiler.output, "}\n", g)?;
        }
        g.check()?;
        Ok(CompiledPolicy {
            document: self,
            text: compiler.output,
            spans: compiler.spans,
            requirements: compiler.requirements,
            dependencies: compiler.dependencies,
        })
    }
}

fn import_path<'a>(
    source: &'a str,
    path: &'a str,
    g: &mut Governor,
) -> Result<(), PolicyCause<'a>> {
    admit_identity(path, g)?;
    g.charge(Resource::Work, size(path.len()).saturating_add(1))?;
    let invalid = |offset| PolicyCause::ImportPath {
        source,
        path,
        offset,
    };
    let Some(rest) = path.strip_prefix("abstractions/") else {
        return Err(invalid(0));
    };
    let mut offset = "abstractions/".len();
    for component in rest.split('/') {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err(invalid(offset));
        }
        if let Some(index) = component
            .bytes()
            .position(|byte| !(byte.is_ascii_alphanumeric() || b"_.+-".contains(&byte)))
        {
            return Err(invalid(offset + index));
        }
        offset += component.len() + 1;
    }
    Ok(())
}
fn equal(left: &str, right: &str, g: &mut Governor) -> Result<bool, StopReason> {
    g.charge(
        Resource::Work,
        size(left.len().min(right.len())).saturating_add(1),
    )?;
    Ok(left == right)
}
fn bind_dependencies<'a>(
    dependencies: &'a [PolicyDependency<'a>],
    g: &mut Governor,
) -> Result<ScratchVec<BoundPolicyDependency<'a>>, PolicyCause<'a>> {
    let mut bound = ScratchVec::new(g);
    for (index, dependency) in dependencies.iter().enumerate() {
        admit_identity(dependency.source, g)?;
        import_path(dependency.source, dependency.path, g)?;
        for previous in &dependencies[..index] {
            if equal(previous.path, dependency.path, g)? {
                return Err(PolicyCause::DependencyConflict {
                    first: previous,
                    second: dependency,
                });
            }
        }
        g.charge(Resource::InputBytes, size(dependency.bytes.len()))?;
        g.charge(
            Resource::Work,
            size(dependency.bytes.len()).saturating_add(1),
        )?;
        let digest = Digest::of(dependency.bytes);
        bound.push(BoundPolicyDependency {
            dependency: *dependency,
            digest,
        })?;
    }
    g.check()?;
    Ok(bound)
}

fn admit_identity(value: &str, g: &mut Governor) -> Result<(), StopReason> {
    g.charge(Resource::InputBytes, size(value.len()))?;
    // Borrowing an identity and reading its length are constant work. Actual
    // lexical scans, comparisons and emitted copies admit their own byte work.
    g.charge(Resource::Work, 1)
}
fn ordered<'a>(
    len: usize,
    key: impl Fn(usize) -> &'a str,
    scope: PolicyDuplicateScope,
    g: &mut Governor,
) -> Result<ScratchVec<usize>, PolicyCause<'a>> {
    let mut indices = ScratchVec::new(g);
    for i in 0..len {
        g.charge(Resource::Work, 1)?;
        admit_identity(key(i), g)?;
        indices.push(i)?;
    }
    for i in 1..len {
        let mut at = i;
        while at > 0 {
            let left = key(indices[at - 1]);
            let right = key(indices[at]);
            g.charge(
                Resource::Work,
                size(left.len().min(right.len())).saturating_add(1),
            )?;
            if left <= right {
                break;
            }
            g.charge(Resource::Work, 1)?;
            indices.as_mut_slice().swap(at - 1, at);
            at -= 1;
        }
    }
    for pair in indices.windows(2) {
        let left = key(pair[0]);
        let right = key(pair[1]);
        g.charge(
            Resource::Work,
            size(left.len().min(right.len())).saturating_add(1),
        )?;
        if left == right {
            return Err(PolicyCause::Duplicate { name: left, scope });
        }
    }
    Ok(indices)
}

struct Compiler<'s, 'a> {
    document: &'s PolicyDocument<'a>,
    output: ScratchString,
    spans: ScratchVec<PolicySpan<'s>>,
    requirements: PolicyRequirements,
    dependencies: ScratchVec<BoundPolicyDependency<'s>>,
}
impl<'s> Compiler<'s, '_> {
    fn append(
        &mut self,
        fragment: &Rendered,
        profile: &'s str,
        hat: Option<&'s str>,
        origin: PolicyOrigin<'s>,
        indent: &str,
        g: &mut Governor,
    ) -> Result<(), PolicyCause<'s>> {
        let source = match origin {
            PolicyOrigin::Caller(id) | PolicyOrigin::Import { source: id, .. } => id,
            PolicyOrigin::Baseline(_) => self.document.input.source,
        };
        self.requirements
            .add(source, fragment.abi, fragment.mediation, g)?;
        g.charge(Resource::Work, size(fragment.text.len()).saturating_add(1))?;
        let start = self.output.len();
        for line in fragment.text.split_inclusive('\n') {
            emit(&mut self.output, indent, g)?;
            emit(&mut self.output, line, g)?;
        }
        let end = self.output.len();
        self.spans.push(PolicySpan {
            profile,
            hat,
            origin,
            start,
            end,
        })?;
        Ok(())
    }
    fn body(
        &mut self,
        body: PolicyBody<'s>,
        profile: &'s str,
        hat: Option<&'s str>,
        indent: &str,
        g: &mut Governor,
    ) -> Result<(), PolicyCause<'s>> {
        admit_identity(body.source, g)?;
        g.charge(Resource::Depth, if hat.is_some() { 2 } else { 1 })?;
        let indices = ordered(
            body.rules.len(),
            |i| body.rules[i].source(),
            PolicyDuplicateScope::Rules,
            g,
        )?;
        self.baselines(profile, hat, indent, g)?;
        self.imports(body.imports, profile, hat, indent, g)?;
        for &index in &indices {
            g.charge(Resource::Work, 1)?;
            let rule = body.rules[index];
            if matches!(rule, PolicyRule::Internet { .. })
                && self.document.input.target == PolicyTarget::Abi3
            {
                return Err(PolicyCause::Target {
                    source: rule.source(),
                    target: self.document.input.target,
                    required_abi: "4.0-ip",
                    mediation: "network_v8/af_inet",
                });
            }
            let rendered = rule.render(g)?;
            self.append(
                &rendered,
                profile,
                hat,
                PolicyOrigin::Caller(rule.source()),
                indent,
                g,
            )?;
        }
        Ok(())
    }
    fn imports(
        &mut self,
        imports: &'s [PolicyImport<'s>],
        profile: &'s str,
        hat: Option<&'s str>,
        indent: &str,
        g: &mut Governor,
    ) -> Result<(), PolicyCause<'s>> {
        for (index, &import) in imports.iter().enumerate() {
            admit_identity(import.source, g)?;
            import_path(import.source, import.path, g)?;
            for previous in &imports[..index] {
                if equal(previous.source, import.source, g)? {
                    return Err(PolicyCause::Duplicate {
                        name: import.source,
                        scope: PolicyDuplicateScope::Imports,
                    });
                }
            }
            let mut bound = None;
            for (index, dependency) in self.dependencies.iter().enumerate() {
                if equal(import.path, dependency.dependency.path, g)? {
                    bound = Some(index);
                    break;
                }
            }
            let dependency = bound.ok_or(PolicyCause::MissingDependency { import })?;
            let start = self.output.len();
            emit(&mut self.output, indent, g)?;
            emit(&mut self.output, "include <", g)?;
            emit(&mut self.output, import.path, g)?;
            emit(&mut self.output, ">\n", g)?;
            self.spans.push(PolicySpan {
                profile,
                hat,
                origin: PolicyOrigin::Import {
                    source: import.source,
                    dependency,
                },
                start,
                end: self.output.len(),
            })?;
            self.requirements
                .first_opaque_import
                .get_or_insert(self.spans.len() - 1);
        }
        Ok(())
    }

    fn baselines(
        &mut self,
        profile: &'s str,
        hat: Option<&'s str>,
        indent: &str,
        g: &mut Governor,
    ) -> Result<(), PolicyCause<'s>> {
        let document = self.document;
        for (path, kind) in [
            (&document.ssh, PolicyBaseline::Ssh),
            (&document.environment, PolicyBaseline::Environment),
        ] {
            let binding_slice = if kind == PolicyBaseline::Ssh {
                &document.home_binding[..]
            } else {
                &[][..]
            };
            let native = FileRule {
                source: "compiler:baseline:file",
                path,
                bindings: binding_slice,
                permissions: document.files,
                execution: Some(FileExecution::Inherit),
                target: None,
                audit: true,
                deny: true,
                owner: false,
            }
            .render(g)
            .map_err(PolicyCause::File)?;
            self.append(
                &Rendered {
                    text: native.text,
                    abi: native.requirement.abi,
                    mediation: native.requirement.mediation,
                },
                profile,
                hat,
                PolicyOrigin::Baseline(kind),
                indent,
                g,
            )?;
        }
        let capabilities = dangerous_capability_denials("compiler:baseline:caps", false, g)
            .map_err(PolicyCause::Privilege)?;
        self.append(
            &Rendered {
                text: capabilities.text,
                abi: capabilities.requirement.abi,
                mediation: capabilities.requirement.mediation,
            },
            profile,
            hat,
            PolicyOrigin::Baseline(PolicyBaseline::Capabilities),
            indent,
            g,
        )?;
        let native = PtraceRule {
            source: "compiler:baseline:ptrace",
            peer: &document.peers,
            bindings: &[],
            access: document.ptrace,
            audit: false,
            deny: true,
        }
        .render(g)
        .map_err(PolicyCause::Ipc)?;
        self.append(
            &Rendered {
                text: native.text,
                abi: native.requirement.abi,
                mediation: native.requirement.mediation,
            },
            profile,
            hat,
            PolicyOrigin::Baseline(PolicyBaseline::Ptrace),
            indent,
            g,
        )?;
        for (operation, kind) in [
            ("mount", PolicyBaseline::Mount),
            ("remount", PolicyBaseline::Remount),
            ("umount", PolicyBaseline::Umount),
            ("pivot_root", PolicyBaseline::PivotRoot),
        ] {
            let native = MountRule {
                source: "compiler:baseline:mount",
                operation,
                audit: false,
                deny: true,
            }
            .render(g)
            .map_err(PolicyCause::Privilege)?;
            self.append(
                &Rendered {
                    text: native.text,
                    abi: native.requirement.abi,
                    mediation: native.requirement.mediation,
                },
                profile,
                hat,
                PolicyOrigin::Baseline(kind),
                indent,
                g,
            )?;
        }
        Ok(())
    }
}
