//! JSON rendering for the capability report (`docs/reference/capability-report.md`).

use serde_json::{Value, json};
use slug_frontend::{CapabilityReport, ForeignCapability, ModuleCapabilities};

/// Renders a capability report as its version 1 JSON document.
#[must_use]
pub fn capability_report_json(report: &CapabilityReport) -> Value {
    json!({
        "version": report.version,
        "entry": report.entry,
        "closed": report.closed,
        "modules": report.modules.iter().map(module_json).collect::<Vec<_>>(),
    })
}

fn module_json(module: &ModuleCapabilities) -> Value {
    let mut value = json!({
        "name": module.name,
        "imports": module.imports,
        "builtins": module.builtins,
        "foreign": module.foreign.iter().map(foreign_json).collect::<Vec<_>>(),
        "resources": module
            .resources
            .iter()
            .map(|resource| json!({
                "name": resource.name,
                "exported": resource.exported,
                "tags": resource.tags,
            }))
            .collect::<Vec<_>>(),
    });
    if let Some(sites) = &module.dynamic_imports {
        value["dynamic_imports"] = sites
            .iter()
            .map(|site| json!({ "line": site.line, "column": site.column }))
            .collect();
    }
    value
}

fn foreign_json(foreign: &ForeignCapability) -> Value {
    let mut value = json!({
        "name": foreign.name,
        "exported": foreign.exported,
        "arity": { "min": foreign.arity.min, "max": foreign.arity.max },
        "tags": foreign.tags,
    });
    if let Some(parameters) = &foreign.parameters {
        value["parameters"] = parameters
            .iter()
            .map(|parameter| {
                json!({
                    "name": parameter.name,
                    "type": parameter.annotation,
                    "variadic": parameter.variadic,
                    "default": parameter.default,
                })
            })
            .collect();
        value["result"] = json!(foreign.result);
    }
    value
}
