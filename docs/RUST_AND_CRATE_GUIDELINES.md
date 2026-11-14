# Rust and Crate Guidelines

Inherited from the shared MidManStudio convention established in mid-engine and DixScript-Rust. Rules only, no decision log here, see those repos' own docs/RUST_AND_CRATE_GUIDELINES.md for the history behind each one.

## 1. Workspace Structure

A real Cargo workspace: [workspace], resolver = "2", members under crates/. Crates that depend on each other use path dependencies. Shared metadata lives in [workspace.package] (edition, rust-version, license-file) and every crate inherits it with edition.workspace = true, etc.

## 2. Feature Gating

A substantial chunk of a crate that not every consumer needs gets a Cargo feature. A small surface is allowed to stay unconditional.

## 3. Unsafe Code

The workspace denies unsafe_code by default. A crate that genuinely needs unsafe opts out explicitly with #![allow(unsafe_code)] at its crate root, with a SAFETY comment on every unsafe block and a Safety doc section on every unsafe fn.

## 4. Versioning

Every crate starts and stays at 0.0.1 for as long as it's in active dev. The first real release jumps straight to 1.0.0.

## 5. CI Triggering

workflow_dispatch only, never push or pull_request for the test/lint workflow. Cache only ~/.cargo/registry/{index,cache} and ~/.cargo/git, never ~/.cargo/bin/. Scope the cache key to runner.os plus the repo name.
