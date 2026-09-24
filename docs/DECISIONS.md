# Decisions

Choices that were made on purpose, so they are not reopened by accident.

## Blocking calls in the SDK

The SDK is synchronous. A command line wants that, a Tauri command runs
on a thread anyway, and a program on tokio can `spawn_blocking`. An async
client doubles the surface and the dependency tree for a handful of
callers; it can come later behind a feature flag.

## ureq, serde, serde_json, and nothing else

An HTTP client with TLS is the one thing not worth writing. Everything
else the SDK needs (percent-encoding, HMAC, SHA-256, base64 in the CLI)
is a screen of code and is here, tested. The argument parser in the CLI
is written for the same reason: clap is excellent and eight times the
size of everything else in the binary.

## A token, not a sign-in flow

A client signs in with a token the person made in the web client, with
exactly the scopes it needs, revocable in one place. A device flow would
be nicer to start with and worse to reason about; it can be added to the
service and picked up here when it exists.

## The token stays on the Rust side of the app

The window is a web view. It gets what it asks for by name and never the
token, so a mistake in the window cannot leak it.

## MPL-2.0 for the SDK and the CLI, AGPL-3.0 for the app

The SDK and the command line should be usable in anyone's program,
including closed ones, and changes to their files should come back:
that is the MPL. The app is a product; improvements to it should stay
open for everyone, including when it is offered as a service: that is
the AGPL.

## Path-style, unversioned API, only additions

The service adds fields and never renames or removes one, so the types
here ignore unknown fields and default missing ones. A breaking change
would come as `/api/v2` and a year of both.
