// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 Span Brain

//! Embeds the 12-character build commit as `RAMFLUX_TUI_BUILD`.
//! `RAMFLUX_GIT_SHA` in the environment wins; without git or the variable it is `unknown`.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=RAMFLUX_GIT_SHA");
    let sha = std::env::var("RAMFLUX_GIT_SHA")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .map_or_else(|| "unknown".to_owned(), |value| value.chars().take(12).collect());
    println!("cargo:rustc-env=RAMFLUX_TUI_BUILD={sha}");
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
    }
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!text.is_empty()).then_some(text)
}
