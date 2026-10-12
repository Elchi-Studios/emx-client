// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A command line `emx` does not understand is refused before anything
//! reaches the service, with exit status 2.

mod common;

use common::emx;

/// Nothing listens here: a command that reaches the network fails with
/// status 1, not 2.
const NOWHERE: &str = "http://127.0.0.1:9";

fn run(args: &[&str]) -> (Option<i32>, String) {
    let out = emx(NOWHERE).args(args).output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn delete_with_an_unknown_switch_deletes_nothing() {
    let (code, err) = run(&["delete", "x1", "--dry-run"]);
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("has no option --dry-run"), "{err}");
}

#[test]
fn send_with_a_misspelt_flag_sends_nothing() {
    for extra in [["--subjct", "Offerte"], ["--attachment", "Offerte.pdf"]] {
        let (code, err) = run(&[
            "send",
            "--from",
            "a@example.ch",
            "--to",
            "b@example.ch",
            "--text",
            "x",
            extra[0],
            extra[1],
        ]);
        assert_eq!(code, Some(2), "{err}");
    }
}

#[test]
fn missing_and_stray_arguments_are_usage_errors() {
    assert_eq!(run(&["read"]).0, Some(2));
    assert_eq!(run(&["read", "a", "b"]).0, Some(2));
    assert_eq!(run(&["frobnicate"]).0, Some(2));
    assert_eq!(run(&["list", "--limit", "0"]).0, Some(2));
    // A well-formed command reaches the network and fails there.
    assert_eq!(run(&["read", "a"]).0, Some(1));
}

#[test]
fn short_help_shows_the_help() {
    let out = emx(NOWHERE).args(["delete", "x1", "-h"]).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("emx, mail from the command line"));
}

/// Under a service manager or in a container there may be no home
/// folder; `EMX_TOKEN` is enough.
#[test]
fn a_token_in_the_environment_needs_no_home() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_emx"))
        .env_clear()
        .env("EMX_TOKEN", "emx_test")
        .env("EMX_BASE_URL", NOWHERE)
        .arg("me")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("home"), "{err}");
    assert!(err.contains("could not reach EMX"), "{err}");
}
