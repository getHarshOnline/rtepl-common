// SPDX-License-Identifier: AGPL-3.0-or-later

//! Consumer-selected restrictions on effect access, exports and trait implementations,
//! applied to the production source of a repository's pure layers. It builds on `syn`,
//! and Weave's architecture test calls it with its own policy. Nothing else in this
//! repository builds on it. The consumer names the layers, paths and types; this
//! module knows none of them and inspects syntax before macro expansion.

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

/// A trait that may not be implemented or derived for a named type.
#[derive(Debug)]
pub struct ForbiddenImplementation<'a> {
    /// Final type name, including uses behind a local type alias.
    pub type_name: &'a str,
    /// Final trait name, such as `From`, `Default` or `Deserialize`.
    pub trait_name: &'a str,
}

/// Rules applied by the caller to one production source file.
#[derive(Debug, Default)]
pub struct PurityPolicy<'a> {
    /// Rust path prefixes forbidden anywhere in this file. For example,
    /// `std::fs`, `std::env`, `std::time::SystemTime::now` or a runtime crate.
    pub forbidden_paths: &'a [&'a str],
    /// Rust path prefixes forbidden in public imports and public type signatures.
    /// Application code may still use these paths privately.
    pub forbidden_reexports: &'a [&'a str],
    /// Type/trait pairs rejected in explicit implementations and derives.
    pub forbidden_implementations: &'a [ForbiddenImplementation<'a>],
}

/// Inspect Rust syntax for the caller's purity rules.
///
/// Imports, renamed imports, extern-crate aliases and local type aliases are
/// followed within this file. All conditional branches are inspected. Glob imports
/// that could hide a forbidden path are rejected. Same-named aliases in different
/// scopes are treated conservatively as all possible targets.
///
/// This is a source check before macro expansion, not a Rust name resolver or a
/// proof about an allowed dependency's code. It does not expand custom macros or
/// follow aliases defined in another file. Consumers must also check dependencies
/// and review generated code and the exposed API.
///
/// # Errors
/// Returns the parser error for invalid Rust; otherwise returns all rule failures.
pub fn check_purity(source: &str, policy: &PurityPolicy<'_>) -> syn::Result<Vec<String>> {
    let file = syn::parse_file(source)?;
    let mut aliases = Aliases::default();
    aliases.visit_file(&file);
    let public_types = aliases.public_types();
    let export_aliases = aliases.export_aliases();
    let mut checker = Checker {
        policy,
        aliases: &aliases.names,
        public_types: &public_types,
        export_aliases: &export_aliases,
        failures: BTreeSet::new(),
        exposed: false,
    };
    checker.visit_file(&file);
    Ok(checker.failures.into_iter().collect())
}

/// Check a crate's source files together, following aliases between those files.
///
/// Each pair contains a display name and source text; this function reads no files.
/// The caller supplies the complete production source selection for one crate and
/// its policy. Failures include their source name. Alias resolution remains
/// conservative and macros are not expanded, as in [`check_purity`].
///
/// # Errors
/// Returns the parser error if any selected source is invalid Rust.
pub fn check_purity_files(
    sources: &[(&str, &str)],
    policy: &PurityPolicy<'_>,
) -> syn::Result<Vec<String>> {
    let files = sources
        .iter()
        .map(|(name, source)| syn::parse_file(source).map(|file| (*name, file)))
        .collect::<syn::Result<Vec<_>>>()?;
    let mut aliases = Aliases::default();
    for (_, file) in &files {
        aliases.visit_file(file);
    }
    let public_types = aliases.public_types();
    let export_aliases = aliases.export_aliases();
    let mut failures = Vec::new();
    for (name, file) in &files {
        let mut checker = Checker {
            policy,
            aliases: &aliases.names,
            public_types: &public_types,
            export_aliases: &export_aliases,
            failures: BTreeSet::new(),
            exposed: false,
        };
        checker.visit_file(file);
        failures.extend(
            checker
                .failures
                .into_iter()
                .map(|failure| format!("{name}: {failure}")),
        );
    }
    Ok(failures)
}

type Names = BTreeMap<String, BTreeSet<String>>;

#[derive(Default)]
struct Aliases {
    names: Names,
    public: BTreeSet<String>,
    contained_types: Names,
}

impl Aliases {
    fn export_aliases(&self) -> Names {
        let mut names = self.names.clone();
        for (name, members) in &self.contained_types {
            names
                .entry(name.clone())
                .or_default()
                .extend(members.iter().cloned());
        }
        names
    }

    fn public_types(&self) -> BTreeSet<String> {
        self.public
            .iter()
            .flat_map(|name| resolved(name, &self.names))
            .map(|name| name.rsplit("::").next().unwrap_or(&name).to_owned())
            .collect()
    }
}

fn path_name(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn imports(tree: &syn::UseTree, prefix: &str, out: &mut Vec<(String, String, bool)>) {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}::{name}")
        }
    };
    match tree {
        syn::UseTree::Path(path) => imports(&path.tree, &join(&path.ident.to_string()), out),
        syn::UseTree::Name(name) => {
            let (name, target) = if name.ident == "self" {
                (
                    prefix.rsplit("::").next().unwrap_or(prefix).to_owned(),
                    prefix.to_owned(),
                )
            } else {
                (name.ident.to_string(), join(&name.ident.to_string()))
            };
            out.push((name, target, false));
        }
        syn::UseTree::Rename(rename) => {
            let target = if rename.ident == "self" {
                prefix.to_owned()
            } else {
                join(&rename.ident.to_string())
            };
            out.push((rename.rename.to_string(), target, false));
        }
        syn::UseTree::Glob(_) => out.push((String::new(), prefix.to_owned(), true)),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                imports(item, prefix, out);
            }
        }
    }
}

impl<'ast> Visit<'ast> for Aliases {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut paths = Vec::new();
        imports(&item.tree, "", &mut paths);
        for (name, target, glob) in paths {
            if !glob {
                if visible(&item.vis) {
                    self.public.insert(name.clone());
                }
                self.names.entry(name).or_default().insert(target);
            }
        }
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        if let Some((_, alias)) = &item.rename {
            self.names
                .entry(alias.to_string())
                .or_default()
                .insert(item.ident.to_string());
        }
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        if visible(&item.vis) {
            self.public.insert(item.ident.to_string());
        }
        if let syn::Type::Path(target) = item.ty.as_ref() {
            self.names
                .entry(item.ident.to_string())
                .or_default()
                .insert(path_name(&target.path));
        }
        let mut paths = TypePaths::default();
        paths.visit_type(&item.ty);
        self.contained_types
            .entry(item.ident.to_string())
            .or_default()
            .extend(paths.0);
        syn::visit::visit_item_type(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        if visible(&item.vis) {
            self.public.insert(item.ident.to_string());
        }
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        if visible(&item.vis) {
            self.public.insert(item.ident.to_string());
        }
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_item_union(&mut self, item: &'ast syn::ItemUnion) {
        if visible(&item.vis) {
            self.public.insert(item.ident.to_string());
        }
        syn::visit::visit_item_union(self, item);
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        if visible(&item.vis) {
            self.public.insert(item.ident.to_string());
        }
        syn::visit::visit_item_trait(self, item);
    }
}

#[derive(Default)]
struct TypePaths(BTreeSet<String>);

impl<'ast> Visit<'ast> for TypePaths {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.0.insert(path_name(path));
        syn::visit::visit_path(self, path);
    }
}

fn prefix(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with("::"))
}

// Include the written name as well as its possible imported targets. A per-route
// visited set bounds cycles, including aliases that add another path component.
fn resolved(path: &str, aliases: &Names) -> BTreeSet<String> {
    fn expand(
        path: String,
        aliases: &Names,
        visited: &BTreeSet<String>,
        out: &mut BTreeSet<String>,
    ) {
        out.insert(path.clone());
        // An alias can be qualified by a crate-local path. Check every matching
        // name conservatively, including names reached through chained aliases.
        let mut suffix = path.as_str();
        loop {
            let (head, tail) = suffix.split_once("::").unwrap_or((suffix, ""));
            if !visited.contains(head) {
                if let Some(targets) = aliases.get(head) {
                    let mut visited = visited.clone();
                    visited.insert(head.to_owned());
                    for target in targets {
                        let target = if tail.is_empty() {
                            target.clone()
                        } else {
                            format!("{target}::{tail}")
                        };
                        expand(target, aliases, &visited, out);
                    }
                }
            }
            if tail.is_empty() {
                break;
            }
            suffix = tail;
        }
    }
    let mut out = BTreeSet::new();
    expand(path.to_owned(), aliases, &BTreeSet::new(), &mut out);
    out
}

struct Checker<'a, 'p> {
    policy: &'a PurityPolicy<'p>,
    aliases: &'a Names,
    public_types: &'a BTreeSet<String>,
    export_aliases: &'a Names,
    failures: BTreeSet<String>,
    exposed: bool,
}

impl Checker<'_, '_> {
    fn public_path(&self, path: &syn::Path) -> bool {
        resolved(&path_name(path), self.aliases).iter().any(|name| {
            self.public_types
                .contains(name.rsplit("::").next().unwrap_or(name))
        })
    }

    fn check_path(&mut self, path: &str, glob: bool) {
        for target in resolved(path, self.aliases) {
            for forbidden in self.policy.forbidden_paths {
                if prefix(&target, forbidden) || (glob && prefix(forbidden, &target)) {
                    self.failures
                        .insert(format!("forbidden path {target} ({forbidden})"));
                }
            }
        }
        if self.exposed {
            for target in resolved(path, self.export_aliases) {
                for forbidden in self.policy.forbidden_reexports {
                    if prefix(&target, forbidden) || (glob && prefix(forbidden, &target)) {
                        self.failures
                            .insert(format!("forbidden public export {target} ({forbidden})"));
                    }
                }
            }
        }
    }

    fn implementation(&mut self, target: &str, trait_name: &str) {
        let targets = resolved(target, self.aliases);
        let traits = resolved(trait_name, self.aliases);
        for rule in self.policy.forbidden_implementations {
            if targets
                .iter()
                .any(|target| target.rsplit("::").next() == Some(rule.type_name))
                && traits
                    .iter()
                    .any(|name| name.rsplit("::").next() == Some(rule.trait_name))
            {
                self.failures.insert(format!(
                    "forbidden {} implementation for {}",
                    rule.trait_name, rule.type_name
                ));
            }
        }
    }

    fn derives(&mut self, name: &syn::Ident, attributes: &[syn::Attribute]) {
        for attribute in attributes {
            self.derive_meta(name, &attribute.meta);
        }
    }

    fn derive_meta(&mut self, name: &syn::Ident, meta: &syn::Meta) {
        let syn::Meta::List(list) = meta else { return };
        if list.path.is_ident("derive") {
            if let Ok(paths) = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            ) {
                for path in paths {
                    self.implementation(&name.to_string(), &path_name(&path));
                }
            }
        } else if list.path.is_ident("cfg_attr") {
            if let Ok(attributes) = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            ) {
                for attribute in attributes.iter().skip(1) {
                    self.derive_meta(name, attribute);
                }
            }
        }
    }

    fn attribute_meta(&mut self, meta: &syn::Meta) {
        self.check_path(&path_name(meta.path()), false);
        if let syn::Meta::List(list) = meta {
            if list.path.is_ident("cfg_attr") {
                if let Ok(attributes) = list.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                ) {
                    for attribute in attributes.iter().skip(1) {
                        self.attribute_meta(attribute);
                    }
                }
            }
        }
    }
}

fn visible(visibility: &syn::Visibility) -> bool {
    !matches!(visibility, syn::Visibility::Inherited)
}

impl<'ast> Visit<'ast> for Checker<'_, '_> {
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        self.attribute_meta(&attribute.meta);
        syn::visit::visit_attribute(self, attribute);
    }

    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        if let Some(qualified) = &expression.qself {
            if let syn::Type::Path(target) = qualified.ty.as_ref() {
                let members = expression
                    .path
                    .segments
                    .iter()
                    .skip(qualified.position)
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                self.check_path(&format!("{}::{members}", path_name(&target.path)), false);
            }
        }
        syn::visit::visit_expr_path(self, expression);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.check_path(&path_name(path), false);
        syn::visit::visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        let mut paths = Vec::new();
        imports(&item.tree, "", &mut paths);
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        for (_, path, glob) in paths {
            self.check_path(&path, glob);
        }
        self.exposed = previous;
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        self.check_path(&item.ident.to_string(), false);
        self.exposed = previous;
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        if let (Some((_, name, _)), syn::Type::Path(target)) = (&item.trait_, item.self_ty.as_ref())
        {
            self.implementation(&path_name(&target.path), &path_name(name));
        }
        let public_target = match item.self_ty.as_ref() {
            syn::Type::Path(target) => self.public_path(&target.path),
            _ => false,
        };
        let public_trait = item
            .trait_
            .as_ref()
            .is_some_and(|(_, path, _)| self.public_path(path));
        let public_member = item.items.iter().any(|member| match member {
            syn::ImplItem::Fn(value) => visible(&value.vis),
            syn::ImplItem::Const(value) => visible(&value.vis),
            syn::ImplItem::Type(value) => visible(&value.vis),
            _ => false,
        });
        let previous = self.exposed;
        let trait_interface = item.trait_.is_some() && (public_target || public_trait);
        self.exposed = trait_interface || (public_target && public_member);
        self.visit_generics(&item.generics);
        self.visit_type(&item.self_ty);
        if let Some((_, path, _)) = &item.trait_ {
            self.visit_path(path);
        }
        self.exposed = trait_interface;
        for member in &item.items {
            self.visit_impl_item(member);
        }
        self.exposed = previous;
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.derives(&item.ident, &item.attrs);
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        self.visit_generics(&item.generics);
        self.exposed = previous;
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.derives(&item.ident, &item.attrs);
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        syn::visit::visit_item_enum(self, item);
        self.exposed = previous;
    }

    fn visit_item_union(&mut self, item: &'ast syn::ItemUnion) {
        self.derives(&item.ident, &item.attrs);
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        self.visit_generics(&item.generics);
        self.exposed = previous;
        syn::visit::visit_item_union(self, item);
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        let previous = self.exposed;
        self.exposed |= visible(&field.vis);
        syn::visit::visit_field(self, field);
        self.exposed = previous;
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        syn::visit::visit_item_type(self, item);
        self.exposed = previous;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        self.visit_signature(&item.sig);
        self.exposed = previous;
        self.visit_block(&item.block);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        let previous = self.exposed;
        self.exposed |= visible(&item.vis);
        self.visit_signature(&item.sig);
        self.exposed = false;
        self.visit_block(&item.block);
        self.exposed = previous;
    }

    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        let previous = self.exposed;
        self.exposed |= visible(&item.vis);
        self.visit_generics(&item.generics);
        self.visit_type(&item.ty);
        self.exposed = false;
        self.visit_expr(&item.expr);
        self.exposed = previous;
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        self.visit_type(&item.ty);
        self.exposed = previous;
        self.visit_expr(&item.expr);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        self.visit_type(&item.ty);
        self.exposed = previous;
        self.visit_expr(&item.expr);
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        let previous = self.exposed;
        self.exposed = visible(&item.vis);
        self.visit_generics(&item.generics);
        for bound in &item.supertraits {
            self.visit_type_param_bound(bound);
        }
        for member in &item.items {
            match member {
                syn::TraitItem::Fn(method) => {
                    for attribute in &method.attrs {
                        self.visit_attribute(attribute);
                    }
                    self.visit_signature(&method.sig);
                    let exposed = self.exposed;
                    self.exposed = false;
                    if let Some(block) = &method.default {
                        self.visit_block(block);
                    }
                    self.exposed = exposed;
                }
                _ => self.visit_trait_item(member),
            }
        }
        self.exposed = previous;
    }
}
