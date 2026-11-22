use rtepl_repository::purity::{check_purity, ForbiddenImplementation, PurityPolicy};

fn pure(source: &str) -> Vec<String> {
    check_purity(
        source,
        &PurityPolicy {
            forbidden_paths: &[
                "std::fs",
                "std::net",
                "std::env",
                "std::time::SystemTime::now",
                "std::time::Instant::now",
                "rand",
                "tokio",
            ],
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn rejects_effect_access_through_direct_grouped_renamed_and_chained_imports() {
    for source in [
        "fn run() { std::fs::read(\"file\"); }",
        "use std::{net as network}; fn run() { network::TcpStream::connect(\"host\"); }",
        "use std as host; use host::env as config; fn run() { config::var(\"name\"); }",
        "use std::time::SystemTime as Clock; fn run() { Clock::now(); }",
        "type Clock = std::time::Instant; fn run() { Clock::now(); }",
        "use std::time::Instant as Clock; fn run() { <Clock>::now(); }",
        "fn run() { <std::time::SystemTime>::now(); }",
        "use std::time::*; fn run() { SystemTime::now(); }",
        "extern crate rand as random; fn run() { random::random::<u32>(); }",
        "#[tokio::main] async fn run() {}",
        "struct Api; #[tokio::test] impl Api {}",
        "#[cfg_attr(feature = \"runtime\", tokio::main)] async fn run() {}",
        "#[cfg(any())] fn disabled() { std::env::vars(); }",
    ] {
        assert!(!pure(source).is_empty(), "accepted: {source}");
    }
}

#[test]
fn accepts_inert_time_values_plain_data_and_prose_without_effect_calls() {
    let source = r#"
        use std::time::{Duration, SystemTime};
        fn compare(now: SystemTime, before: SystemTime) -> Duration {
            now.duration_since(before).unwrap()
        }
        // std::env::var("example");
        const HELP: &str = "std::fs::read and tokio::spawn";
    "#;
    assert!(pure(source).is_empty());
    assert!(check_purity(
        "fn f() { std::fs::read(\"file\"); }",
        &PurityPolicy::default()
    )
    .unwrap()
    .is_empty());
    assert!(check_purity("not Rust", &PurityPolicy::default()).is_err());
}

#[test]
fn rejects_ports_and_adapters_in_exports_but_allows_private_application_use() {
    let policy = PurityPolicy {
        forbidden_reexports: &["product_ports", "storage_adapter"],
        ..Default::default()
    };
    for source in [
        "pub use product_ports::Store;",
        "pub use storage_adapter::{Database as Store};",
        "use product_ports as ports; pub use ports::*;",
        "use product_ports::Store as Handle; pub use Handle as PublicStore;",
        "mod private { pub use product_ports::Store as Handle; } pub use private::Handle;",
        "pub extern crate storage_adapter as database;",
        "pub type Store = product_ports::Store;",
        "use product_ports::Store as Handle; pub fn connect() -> Handle { todo!() }",
        "pub struct Api { pub store: product_ports::Store }",
        "pub struct Api<T: product_ports::Store> { value: T }",
        "pub union Api<T: product_ports::Store> { value: T }",
        "pub const STORE: product_ports::Store = todo!();",
        "pub enum Api { Store(product_ports::Store) }",
        "pub trait Api: product_ports::Store { fn store() -> storage_adapter::Database; }",
        "struct Api; impl Api { pub fn store() -> product_ports::Store { todo!() } }",
    ] {
        let failures = check_purity(source, &policy).unwrap();
        assert!(
            failures
                .iter()
                .any(|s| s.contains("forbidden public export")),
            "accepted: {source}"
        );
    }
    let private = "use product_ports::Store; struct App { store: Store } pub fn run() { storage_adapter::open(); }";
    assert!(check_purity(private, &policy).unwrap().is_empty());
}

#[test]
fn rejects_explicit_and_derived_implementations_on_the_target_type_only() {
    let rules = [
        ForbiddenImplementation {
            type_name: "Refusal",
            trait_name: "From",
        },
        ForbiddenImplementation {
            type_name: "Outcome",
            trait_name: "From",
        },
        ForbiddenImplementation {
            type_name: "CommitReceipt",
            trait_name: "From",
        },
        ForbiddenImplementation {
            type_name: "CommitReceipt",
            trait_name: "Default",
        },
        ForbiddenImplementation {
            type_name: "CommitReceipt",
            trait_name: "Deserialize",
        },
    ];
    let policy = PurityPolicy {
        forbidden_implementations: &rules,
        ..Default::default()
    };
    for source in [
        "impl From<Error> for Refusal { fn from(e: Error) -> Self { todo!() } }",
        "impl<T> std::convert::From<T> for crate::Outcome<T> { fn from(t: T) -> Self { todo!() } }",
        "use std::default::Default as Fallback; impl Fallback for CommitReceipt {}",
        "type Receipt = CommitReceipt; impl From<Data> for Receipt {}",
        "#[derive(serde::Deserialize, Default)] struct CommitReceipt { id: u64 }",
        "#[derive(Default)] enum CommitReceipt { #[default] Empty }",
        "#[cfg_attr(feature = \"decode\", derive(serde::Deserialize))] struct CommitReceipt;",
        "#[cfg_attr(any(), cfg_attr(all(), derive(Default)))] struct CommitReceipt;",
        "impl<'de> serde::Deserialize<'de> for CommitReceipt {}",
    ] {
        assert!(
            !check_purity(source, &policy).unwrap().is_empty(),
            "accepted: {source}"
        );
    }
    assert!(check_purity("#[derive(Default, serde::Deserialize)] struct CommitReport; impl From<CommitReceipt> for CommitReport {}", &policy).unwrap().is_empty());
}

#[test]
fn import_cycles_terminate_and_matching_uses_path_boundaries() {
    assert!(
        pure("use a as b; use b as a; fn f() { a::run(); std::filesystem::read(); } ").is_empty()
    );
    assert!(pure("use self::again as again; fn f() { again::run(); }").is_empty());
}

#[test]
fn crate_source_selection_follows_aliases_between_files() {
    use rtepl_repository::purity::check_purity_files;
    let sources = [
        ("lib.rs", "use product_ports::Store as Handle; mod service;"),
        (
            "service.rs",
            "use super::Handle as Local; pub fn expose() -> Local { todo!() }",
        ),
    ];
    let policy = PurityPolicy {
        forbidden_reexports: &["product_ports"],
        ..Default::default()
    };
    let failures = check_purity_files(&sources, &policy).unwrap();
    assert!(failures
        .iter()
        .any(|failure| failure.starts_with("service.rs:")
            && failure.contains("forbidden public export")));
}

#[test]
fn public_impl_bounds_and_associated_types_cannot_expose_ports() {
    let policy = PurityPolicy {
        forbidden_reexports: &["product_ports"],
        ..Default::default()
    };
    for source in [
        "pub struct Api; impl Iterator for Api { type Item = product_ports::Store; fn next(&mut self) -> Option<Self::Item> { None } }",
        "pub struct Api<T>(T); impl<T: product_ports::Store> Api<T> { pub fn new(x: T) -> Self { Self(x) } }",
        "pub struct Api<T>(T); impl<T> Api<T> where T: product_ports::Store { pub fn new(x: T) -> Self { Self(x) } }",
        "struct Inner; pub type Api = Inner; impl Iterator for Inner { type Item = product_ports::Store; fn next(&mut self) -> Option<Self::Item> { None } }",
        "pub trait Api { type Item; } struct Private; impl Api for Private { type Item = product_ports::Store; }",
    ] {
        assert!(check_purity(source, &policy).unwrap().iter().any(|failure| failure.contains("forbidden public export")), "accepted: {source}");
    }
    let bodies = "pub struct Api; impl Iterator for Api { type Item = u8; fn next(&mut self) -> Option<Self::Item> { product_ports::read(); None } } impl Api { pub fn run() { product_ports::read(); } fn private() -> product_ports::Store { todo!() } }";
    assert!(check_purity(bodies, &policy).unwrap().is_empty());
    let private = "struct Private; impl Iterator for Private { type Item = product_ports::Store; fn next(&mut self) -> Option<Self::Item> { None } }";
    assert!(check_purity(private, &policy).unwrap().is_empty());
}

#[test]
fn public_signatures_follow_the_complete_type_behind_private_aliases() {
    let policy = PurityPolicy {
        forbidden_reexports: &["product_ports"],
        ..Default::default()
    };
    for alias in [
        "Option<product_ports::Store>",
        "(u8, product_ports::Store)",
        "&'static product_ports::Store",
        "fn(u8) -> product_ports::Store",
        "Option<Box<(u8, product_ports::Store)>>",
    ] {
        let source = format!(
            "type Hidden = {alias}; type Handle = Hidden; pub fn expose() -> Handle {{ todo!() }}"
        );
        assert!(
            check_purity(&source, &policy)
                .unwrap()
                .iter()
                .any(|failure| failure.contains("forbidden public export")),
            "accepted: {source}"
        );
    }
    let rules = [ForbiddenImplementation {
        type_name: "CommitReceipt",
        trait_name: "Default",
    }];
    let implementation = PurityPolicy {
        forbidden_implementations: &rules,
        ..Default::default()
    };
    assert!(
        check_purity(
            "type MaybeReceipt = Option<CommitReceipt>; impl Default for MaybeReceipt {}",
            &implementation
        )
        .unwrap()
        .is_empty(),
        "a contained type is not the implementation's target type"
    );
}
