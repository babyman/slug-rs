//! The capability report defined in `docs/reference/capability-report.md`.
//!
//! This module owns the report's content; hosts own its JSON rendering.

use slug_loader::{ModuleLoadError, ModuleResolver};
use slug_vm::cslug::Artifact;

use crate::module::ArtifactSource;
use crate::{ModuleGraph, foreign_declarations, source::import_scan};

/// What a program can touch outside its own Slug code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityReport {
    pub version: u32,
    /// The entry module's name.
    pub entry: String,
    /// Whether the module list bounds every module the program can import.
    /// Only a compiled artifact is closed; source can import dynamically.
    pub closed: bool,
    /// Every module in the program, entry first, then in discovery order.
    pub modules: Vec<ModuleCapabilities>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleCapabilities {
    pub name: String,
    /// Modules this module imports by literal name.
    pub imports: Vec<String>,
    /// `import` calls whose module names are computed at runtime. `None` for
    /// a closed report.
    pub dynamic_imports: Option<Vec<ReportLocation>>,
    /// Host-supplied globals this module references, such as `println` or
    /// `cfg`.
    pub builtins: Vec<String>,
    pub foreign: Vec<ForeignCapability>,
    pub resources: Vec<ResourceCapability>,
}

/// A position within the enclosing module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportLocation {
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignCapability {
    pub name: String,
    pub exported: bool,
    pub arity: ReportArity,
    /// Declared parameters, present when the report was built from source.
    pub parameters: Option<Vec<ReportParameter>>,
    /// Declared result annotation, when built from source and annotated.
    pub result: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportArity {
    pub min: usize,
    /// `None` for a variadic declaration.
    pub max: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportParameter {
    pub name: String,
    /// The type annotation, or `None` when it is omitted.
    pub annotation: Option<String>,
    pub variadic: bool,
    pub default: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceCapability {
    pub name: String,
    pub exported: bool,
    pub tags: Vec<String>,
}

/// Reports a compiled artifact. Its import table is closed, so the report
/// lists every module the program can reach, but carries no foreign
/// parameter details because artifacts do not retain them.
#[must_use]
pub fn capability_report(artifact: &Artifact) -> CapabilityReport {
    build(artifact, None, true)
}

impl ModuleGraph {
    /// Compiles `source` and its static imports through `resolver` and reports
    /// their capabilities.
    ///
    /// # Errors
    ///
    /// Returns checked loader or source errors when a static dependency is
    /// unavailable or invalid.
    pub fn capability_report(
        &self,
        resolver: &dyn ModuleResolver,
        path: &str,
        source: &str,
    ) -> Result<CapabilityReport, ModuleLoadError> {
        let (artifact, sources) = self.compile_artifact_with_sources(resolver, path, source)?;
        let facts = sources
            .iter()
            .map(SourceFacts::read)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(build(&artifact, Some(&facts), false))
    }
}

/// Details only the source text retains.
struct SourceFacts {
    dynamic_imports: Vec<ReportLocation>,
    foreign: Vec<slug_loader::ForeignDeclaration>,
}

impl SourceFacts {
    fn read(source: &ArtifactSource) -> Result<Self, ModuleLoadError> {
        let error = |error: crate::SourceError| ModuleLoadError::Compile {
            location: source.diagnostic_name.clone(),
            error: Box::new((&error).into()),
        };
        let scan = import_scan(&source.diagnostic_name, &source.text).map_err(error)?;
        Ok(Self {
            dynamic_imports: scan
                .dynamic
                .iter()
                .map(|span| ReportLocation {
                    line: span.line,
                    column: span.column,
                })
                .collect(),
            foreign: foreign_declarations(&source.diagnostic_name, &source.text).map_err(error)?,
        })
    }
}

fn build(artifact: &Artifact, facts: Option<&[SourceFacts]>, closed: bool) -> CapabilityReport {
    let order = std::iter::once(artifact.root)
        .chain((0..artifact.modules.len()).filter(|index| *index != artifact.root));
    let modules = order
        .map(|index| module_capabilities(artifact, index, facts.map(|facts| &facts[index])))
        .collect();
    CapabilityReport {
        version: 1,
        entry: artifact.modules[artifact.root].name.clone(),
        closed,
        modules,
    }
}

fn module_capabilities(
    artifact: &Artifact,
    index: usize,
    facts: Option<&SourceFacts>,
) -> ModuleCapabilities {
    let module = &artifact.modules[index];
    let program = &module.program;

    let mut imports = artifact
        .imports
        .iter()
        .filter(|edge| edge.importer == index)
        .map(|edge| edge.name.clone())
        .collect::<Vec<_>>();
    imports.sort();
    imports.dedup();

    let mut builtins = program
        .global_references()
        .iter()
        .filter(|name| !program.bindings().contains(name))
        .cloned()
        .collect::<Vec<_>>();
    builtins.sort();

    let mut source_foreign = facts.map(|facts| facts.foreign.iter());
    let mut foreign = Vec::new();
    let mut resources = Vec::new();
    for declaration in program.declarations() {
        let tags = declaration
            .tags
            .iter()
            .map(|tag| tag.name.clone())
            .collect();
        let name = declaration.bindings.first().cloned().unwrap_or_default();
        if declaration.foreign {
            // Source and compiled declarations list foreign members in the
            // same top-level order, so each takes the next source member.
            let source = source_foreign
                .as_mut()
                .and_then(Iterator::next)
                .filter(|source| source.name == name);
            let (min, max) = declaration.foreign_arity.unwrap_or((0, None));
            foreign.push(ForeignCapability {
                name,
                exported: declaration.exported,
                arity: ReportArity { min, max },
                parameters: source.map(|source| {
                    source
                        .parameters
                        .iter()
                        .map(|parameter| ReportParameter {
                            name: parameter.name.clone(),
                            annotation: parameter.annotation.clone(),
                            variadic: parameter.variadic,
                            default: parameter.has_default,
                        })
                        .collect()
                }),
                result: source.and_then(|source| source.result.clone()),
                tags,
            });
        } else if let Some(resource) = &declaration.resource_type {
            resources.push(ResourceCapability {
                name: resource.clone(),
                exported: declaration.exported,
                tags,
            });
        }
    }

    ModuleCapabilities {
        name: module.name.clone(),
        imports,
        dynamic_imports: facts.map(|facts| facts.dynamic_imports.clone()),
        builtins,
        foreign,
        resources,
    }
}
