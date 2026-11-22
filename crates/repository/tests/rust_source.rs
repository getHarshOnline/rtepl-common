use rtepl_repository::rust_source::{check_rust_source, RustSourcePolicy};

#[test]
fn finds_nested_tests_and_unchecked_calls_while_ignoring_prose() {
    let policy = RustSourcePolicy {
        test_functions: true,
        inline_test_modules: true,
        forbidden_methods: &["unwrap", "unwrap_err"],
    };
    let source = r#"
        // #[test] fn example() { value.unwrap(); }
        const TEXT: &str = "mod tests { } .unwrap_err()";
        mod tests;
        mod nested { mod tests { #[test] fn actual() { value.unwrap(); value.unwrap_err(); } } }
    "#;
    assert_eq!(check_rust_source(source, &policy).unwrap().len(), 4);
    assert!(check_rust_source(source, &RustSourcePolicy::default())
        .unwrap()
        .is_empty());
    assert!(check_rust_source("not valid Rust", &policy).is_err());
}

#[test]
fn each_rule_is_selected_by_the_consumer() {
    let source = "#[test] fn example() { value.unwrap(); } mod tests {}";
    for policy in [
        RustSourcePolicy {
            test_functions: true,
            ..Default::default()
        },
        RustSourcePolicy {
            inline_test_modules: true,
            ..Default::default()
        },
        RustSourcePolicy {
            forbidden_methods: &["unwrap"],
            ..Default::default()
        },
    ] {
        assert_eq!(check_rust_source(source, &policy).unwrap().len(), 1);
    }
}
