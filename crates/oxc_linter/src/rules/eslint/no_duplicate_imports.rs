use std::collections::HashMap;

use oxc_ast::{
    ast::{
        ExportAllDeclaration, ExportNamedDeclaration, ImportDeclaration, ImportDeclarationSpecifier,
    },
    AstKind,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{context::LintContext, rule::Rule};

fn no_duplicate_imports_diagnostic(module_name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'{}' import is duplicated", module_name))
        .with_help("Merge the duplicated import into a single import statement")
        .with_label(span)
}

fn no_duplicate_exports_diagnostic(module_name: &str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("'{}' export is duplicated", module_name))
        .with_help("Merge the duplicated exports into a single export statement")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoDuplicateImports {
    include_exports: bool,
}

declare_oxc_lint!(
    /// ### What it does
    /// Disallow duplicate module imports
    ///
    /// ### Why is this bad?
    /// Using a single import statement per module will make the code clearer because you can see everything being imported from that module on one line.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// import { merge } from 'module';
    /// import something from 'another-module';
    /// import { find } from 'module';
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// import { merge, find } from 'module';
    /// import something from 'another-module';
    /// ```
    NoDuplicateImports,
    nursery,
    pending);

#[derive(Debug, Clone)]
enum DeclarationType {
    Import,
    Export,
}

#[derive(Debug, Clone)]
enum Specifier {
    Named,
    Default,
    Namespace,
    All,
}

#[derive(Debug, Clone)]
struct ModuleEntry {
    specifier: Specifier,
    declaration_type: DeclarationType,
}

impl Rule for NoDuplicateImports {
    fn from_configuration(value: serde_json::Value) -> Self {
        let Some(value) = value.get(0) else { return Self { include_exports: false } };
        Self {
            include_exports: value
                .get("includeExports")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }
    }

    fn run_once(&self, ctx: &LintContext) {
        let semantic = ctx.semantic();
        let nodes = semantic.nodes();

        let mut modules: HashMap<String, Vec<ModuleEntry>> = HashMap::new();

        for node in nodes {
            match node.kind() {
                AstKind::ImportDeclaration(import_decl) => {
                    handle_import(import_decl, &mut modules, ctx);
                }
                AstKind::ExportNamedDeclaration(export_decl) if self.include_exports => {
                    handle_export(export_decl, &mut modules, ctx);
                }
                AstKind::ExportAllDeclaration(export_decl) if self.include_exports => {
                    handle_export_all(export_decl, &mut modules, ctx);
                }
                _ => {}
            }
        }
    }
}

fn handle_import(
    import_decl: &ImportDeclaration,
    modules: &mut HashMap<String, Vec<ModuleEntry>>,
    ctx: &LintContext,
) {
    let source = &import_decl.source;
    let module_name = source.value.to_string();
    let mut specifier = Specifier::All;

    if let Some(specifiers) = &import_decl.specifiers {
        let has_namespace = specifiers.iter().any(|s| match s {
            ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => false,
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => true,
            _ => false,
        });

        specifier = if specifiers.iter().any(|s| match s {
            ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => true,
            _ => false,
        }) {
            Specifier::Default
        } else if specifiers.iter().any(|s| match s {
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => true,
            _ => false,
        }) {
            Specifier::Namespace
        } else {
            Specifier::Named
        };

        if has_namespace {
            return;
        }
    }

    if let Some(existing_modules) = modules.get(&module_name) {
        if existing_modules.iter().any(|entry| {
            matches!(entry.declaration_type, DeclarationType::Import)
                || matches!(
                    (entry.declaration_type.clone(), entry.specifier.clone()),
                    (DeclarationType::Export, Specifier::All)
                )
        }) {
            ctx.diagnostic(no_duplicate_imports_diagnostic(&module_name, import_decl.span));
            return;
        }
    }

    let entry = ModuleEntry { declaration_type: DeclarationType::Import, specifier };
    modules.entry(module_name.clone()).or_default().push(entry);
}

fn handle_export(
    export_decl: &ExportNamedDeclaration,
    modules: &mut HashMap<String, Vec<ModuleEntry>>,
    ctx: &LintContext,
) {
    let source = match &export_decl.source {
        Some(source) => source,
        None => return,
    };
    let module_name = source.value.to_string();

    if let Some(existing_modules) = modules.get(&module_name) {
        if existing_modules.iter().any(|entry| {
            matches!(entry.declaration_type, DeclarationType::Export)
                || matches!(entry.declaration_type, DeclarationType::Import)
        }) {
            ctx.diagnostic(no_duplicate_exports_diagnostic(&module_name, export_decl.span));
        }
    }

    modules.entry(module_name).or_default().push(ModuleEntry {
        declaration_type: DeclarationType::Export,
        specifier: Specifier::Named,
    });
}

fn handle_export_all(
    export_decl: &ExportAllDeclaration,
    modules: &mut HashMap<String, Vec<ModuleEntry>>,
    ctx: &LintContext,
) {
    let source = &export_decl.source;
    let module_name = source.value.to_string();

    let exported_name = export_decl.exported.clone();

    if let Some(existing_modules) = modules.get(&module_name) {
        if existing_modules.iter().any(|entry| {
            matches!(
                (&entry.declaration_type, &entry.specifier),
                (DeclarationType::Import, Specifier::All)
            ) || matches!(
                (&entry.declaration_type, &entry.specifier),
                (DeclarationType::Export, Specifier::All)
            )
        }) {
            ctx.diagnostic(no_duplicate_exports_diagnostic(&module_name, export_decl.span));
        }

        if exported_name.is_none() {
            return;
        }

        if existing_modules.iter().any(|entry| {
            matches!(
                (&entry.declaration_type, &entry.specifier),
                (DeclarationType::Import, Specifier::Default)
            )
        }) {
            ctx.diagnostic(no_duplicate_exports_diagnostic(&module_name, export_decl.span));
        }
    }

    modules
        .entry(module_name)
        .or_default()
        .push(ModuleEntry { declaration_type: DeclarationType::Export, specifier: Specifier::All });
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        (
            r#"import os from "os";
    		import fs from "fs";"#,
            None,
        ),
        (r#"import { merge } from "lodash-es";"#, None),
        (r#"import _, { merge } from "lodash-es";"#, None),
        (r#"import * as Foobar from "async";"#, None),
        (r#"import "foo""#, None),
        (
            r#"import os from "os";
    		export { something } from "os";"#,
            None,
        ),
        (
            r#"import * as bar from "os";
    		import { baz } from "os";"#,
            None,
        ),
        (
            r#"import foo, * as bar from "os";
    		import { baz } from "os";"#,
            None,
        ),
        (
            r#"import foo, { bar } from "os";
    		import * as baz from "os";"#,
            None,
        ),
        (
            r#"import os from "os";
    		export { hello } from "hello";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import os from "os";
    		export * from "hello";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import os from "os";
    		export { hello as hi } from "hello";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import os from "os";
    		export default function(){};"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import { merge } from "lodash-es";
    		export { merge as lodashMerge }"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"export { something } from "os";
    		export * as os from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import { something } from "os";
    		export * as os from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import * as os from "os";
    		export { something } from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import os from "os";
    		export * from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"export { something } from "os";
    		export * from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
    ];

    let fail = vec![
        (
            r#"import "fs";
        import "fs""#,
            None,
        ),
        (
            r#"import { merge } from "lodash-es";
        import { find } from "lodash-es";"#,
            None,
        ),
        (
            r#"import { merge } from "lodash-es";
          import _ from "lodash-es";"#,
            None,
        ),
        (
            r#"import os from "os";
          import { something } from "os";
          import * as foobar from "os";"#,
            None,
        ),
        (
            r#"import * as modns from "lodash-es";
          import { merge } from "lodash-es";
          import { baz } from "lodash-es";"#,
            None,
        ),
        (
            r#"export { os } from "os";
          export { something } from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import os from "os";
          export { os as foobar } from "os";
          export { something } from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import os from "os";
          export { something } from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import os from "os";
        export * as os from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"export * as os from "os";
        import os from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        // (
        //     r#"import * as modns from "mod";
        // export * as  modns from "mod";"#,
        //     Some(serde_json::json!([{ "includeExports": true }])),
        // ),
        (
            r#"export * from "os";
        export * from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
        (
            r#"import "os";
        export * from "os";"#,
            Some(serde_json::json!([{ "includeExports": true }])),
        ),
    ];

    Tester::new(NoDuplicateImports::NAME, pass, fail).test_and_snapshot();
}
