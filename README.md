# rtepl-common

Small Rust libraries shared by products that need the same mechanics. Each consumer keeps its own data, rules and runtime. `rtepl-repository` inventories source files, checks local Cargo dependencies and inspects production Rust syntax. `rtepl-filesystem` supplies Unix file observations and read-only opening without following a final symbolic link. Consumers supply their own policy.

Pale, Gazette and Weave use `rtepl-repository` in their repository checks, and Pale and Gazette open files through `rtepl-filesystem`. RawThoughts Enterprises Private Limited (RTEPL) builds those products and maintains these libraries. Neither crate is published to a registry: a consumer adds the one it needs as a Git dependency on this repository, pinned to a full commit.

## Build and test

Use Rust 1.94 or later. From this repository:

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
```

The last command builds the API reference under `target/doc/`, which stays out of Git. This README is the manual: the paragraphs below say what each library does and what it leaves to its caller.

The source inventory reports symbolic links without traversing directory entries through them. Callers choose which entries they read and apply their own dependency, naming and layout rules. It is an inventory helper; use a confined filesystem reader when a security-sensitive operation must resist concurrent filesystem changes.

`dependencies::check_dependencies` reads Cargo's metadata output and compares all declared local normal and build dependencies with an exact graph, including optional and target-specific declarations. It checks that paths remain inside the repository, fixture packages stay in their declared homes and only enter dev dependencies, and expressly forbidden edges never occur. Each repository keeps the policy and the Cargo invocation that supplies locked metadata. Registry and Git dependencies are outside this local graph check.

`dependencies::check_external_dependencies` adds an optional allowlist for registry and Git dependencies, keyed by each consumer's package names. An empty set allows none; an absent package is outside the check. It checks all declared normal and build dependencies, including renamed, optional and target-specific ones. `check_external_dependencies_in_scope` also lets the consumer choose `DependencyScope::AllDeclarations` to include dev dependencies in that allowlist. Use either alongside the local graph check. Allowed packages' transitive dependencies and behavior still need their own review.

`rust_source::check_rust_source` detects ordinary test functions, inline test modules and caller-named unchecked methods in parsed Rust. Its policy selects each rule separately. It inspects syntax before macro expansion; richer attribute, generated-code and product API rules stay with the consumer.

`purity::check_purity_files` checks a crate's selected source files against its own `PurityPolicy`: forbidden paths, paths that must not appear in public exports or type signatures, and forbidden type/trait implementations or derives. For example, a consumer can forbid filesystem and network access or clock reads in its pure layers, keep ports out of its public facade, and prohibit decoding or conversion into a protected result type. The consumer names those layers, paths and types; the shared library knows none of them. Imports, renamed imports and type aliases are followed conservatively across the supplied files, and globs that could hide forbidden paths are rejected. `check_purity` checks one file. Both inspect syntax before macro expansion, so neither establishes purity of custom generated code or an allowed dependency. Keep those checks and the review of the complete public API with the consumer.

`rtepl_filesystem::unix` compares device, inode, mode, length and change timestamps through `FileIdentity`. `open_readonly_nofollow` returns a read-only descriptor with close-on-exec and an explicit choice of blocking mode. Callers still check regular-file status, executable permissions, ownership, content digests and later changes as their operation requires. The helper follows ancestor directories and is not a confined path resolver. Its FIFO test uses the system's `mkfifo` command on macOS and Linux.

## Licence

[AGPL-3.0-or-later](LICENSE). The repository-check and file-opening mechanics originate in Pale and Gazette, under the same licence. Commercial use is permitted under the licence. Alternative terms require a separate written agreement with RawThoughts Enterprises Private Limited; this repository grants none. See [NOTICE](NOTICE) for ownership.
