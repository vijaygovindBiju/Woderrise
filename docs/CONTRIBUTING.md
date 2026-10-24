# Contributing Guidelines

> **Document ID:** SPEC-ENG-CON-035  
> **Status:** Approved Architectural Blueprint  
> **Version:** 1.0.0  
> **Last Updated:** 2026-10-24  

---

## 1. Welcoming Contributions

We welcome community contributions across two primary avenues:
1. **Core Development:** Contributing to the Rust engine, D-Bus daemon, CLI, or Libadwaita GUI frontend.
2. **Package Ecosystem:** Publishing new themes, wallpapers, plugins, and complete desktop rices to the community registry.

---

## 2. Core Code Standards & Rust Practices

All code contributions must adhere to modern Rust development best practices:

### 2.1 Formatting & Linting
- **Formatting:** Code must be formatted using standard `rustfmt` with zero manual deviations:
  ```bash
  cargo fmt --all -- --check
  ```
- **Clippy:** Pull requests must pass `cargo clippy` without warnings under strict settings:
  ```bash
  cargo clippy --workspace --all-targets -- -D warnings
  ```

### 2.2 Error Handling & Unwraps
- **No Unwraps in Core Engine:** The use of `.unwrap()` or `.expect()` in `woderrise-core` or `woderrise-dbus` is **strictly forbidden**. All fallible operations must return a strongly typed `Result<T, WoderriseError>`.
- Use the `?` operator and context wrappers (`anyhow::Context` or `thiserror`).

### 2.3 Documentation Integrity
- All public types, traits, functions, and error variants must include comprehensive doc comments (`///`).

---

## 3. Pull Request (PR) Workflow

1. **Create an Issue:** Major architectural or functional changes must begin with an issue or RFC discussion before code is written.
2. **Feature Branch:** Create a branch off `main`: `feature/my-new-feature` or `fix/issue-description`.
3. **Write Comprehensive Tests:** Every new feature or bug fix must include corresponding unit and integration tests. Bug fixes must provide a regression test proving the failure.
4. **Submit PR:** Complete the PR template, referencing relevant issue numbers and summarizing the architectural impact.
5. **CI Approval:** All automated CI checks (Clippy, Format, Unit Tests, Fault-Injection, Security Audit) must pass before review.

---

## 4. Package Ecosystem Contribution Rules

When submitting a new package manifest to `github.com/woderrise/registry`:
1. **Valid Reverse-DNS ID:** Package IDs must strictly follow reverse-DNS convention (`com.creator.package-name`).
2. **Explicit SPDX License:** A recognized, valid SPDX license identifier is mandatory.
3. **Cryptographic Signatures:** Manifests must be accompanied by an Ed25519 signature from the creator.
4. **Zero Malicious Scripts:** Shell scripts must be avoided whenever declarative manifest operations (`files`, `config_injections`, `service_triggers`) suffice.
5. **Quality Previews:** Packages must provide clear, un-distorted HiDPI screenshots (minimum 1920x1080 resolution).
