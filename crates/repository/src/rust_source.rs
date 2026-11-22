// SPDX-License-Identifier: AGPL-3.0-or-later

//! Syntax checks for production Rust, with the rules chosen by its repository: whether
//! test functions, inline test modules or named methods may appear. It builds on `syn`,
//! and the repository checks of Gazette and Weave call it with their own policies.
//! Rules about attributes, generated code and product APIs stay with the caller.

use syn::visit::Visit;

/// Syntax patterns a repository rejects in production source.
#[derive(Debug, Default)]
pub struct RustSourcePolicy<'a> {
    /// Reject ordinary `#[test]` functions, including those in nested modules.
    pub test_functions: bool,
    /// Reject an inline module named `tests`; external test modules stay allowed.
    pub inline_test_modules: bool,
    /// Reject these method names, such as `unwrap` and `unwrap_err`.
    pub forbidden_methods: &'a [&'a str],
}

/// Inspect parsed Rust without treating comments and string contents as code.
///
/// These checks inspect syntax before macro expansion. They do not classify
/// custom test attributes or generated code; consumers keep those extra rules.
///
/// # Errors
/// Returns the parser error for invalid Rust; otherwise returns all rule failures.
pub fn check_rust_source(source: &str, policy: &RustSourcePolicy<'_>) -> syn::Result<Vec<String>> {
    let file = syn::parse_file(source)?;
    let mut visitor = SourceVisitor {
        policy,
        failures: Vec::new(),
    };
    visitor.visit_file(&file);
    Ok(visitor.failures)
}

struct SourceVisitor<'a, 'p> {
    policy: &'a RustSourcePolicy<'p>,
    failures: Vec<String>,
}

impl<'ast> Visit<'ast> for SourceVisitor<'_, '_> {
    fn visit_expr_method_call(&mut self, expression: &'ast syn::ExprMethodCall) {
        if self
            .policy
            .forbidden_methods
            .contains(&expression.method.to_string().as_str())
        {
            self.failures
                .push(format!("unchecked {} call", expression.method));
        }
        syn::visit::visit_expr_method_call(self, expression);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if self.policy.test_functions
            && item
                .attrs
                .iter()
                .any(|attribute| attribute.path().is_ident("test"))
        {
            self.failures
                .push(format!("inline test {}", item.sig.ident));
        }
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if self.policy.inline_test_modules && item.ident == "tests" && item.content.is_some() {
            self.failures.push("inline test module".to_owned());
        }
        syn::visit::visit_item_mod(self, item);
    }
}
