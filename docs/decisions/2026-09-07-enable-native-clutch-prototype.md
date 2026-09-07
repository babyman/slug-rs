---
title: ADR-000 Enable the native clutch prototype by default
date: 2026-09-07
---

## Status

Accepted for the local version-0 experiment.

## Context

The installed `slug.io.fs` clutch already selects the prototype native loader
in ordinary CLI execution, but the dynamic-module regression suite still
required a Cargo feature. That made the test boundary imply that native
clutches were optional or disabled despite being the intended local runtime
path.

## Decision

Remove the `ffi-prototype` Cargo feature and run its dynamic-module tests in
the default test suite. The version-0 loader remains private to manifest-
selected local clutches and may change incompatibly while the language is
pre-release.

This supersedes the feature-gating portions of
[`2026-09-07-test-built-filesystem-clutch-ffi.md`](2026-09-07-test-built-filesystem-clutch-ffi.md)
and [`2026-09-06-cross-platform-c-ffi-prototype-loader.md`](2026-09-06-cross-platform-c-ffi-prototype-loader.md).

## Consequences

### Positive

Default validation now proves the same dynamic native-clutch path exposed by
the local CLI. Experimental describes compatibility posture, not whether the
implementation is compiled or tested.

### Negative

Every supported development environment must provide the native fixture build
toolchain required by the dynamic-module tests.

### Neutral

This does not create an ABI-v1 promise, permit third-party compatibility, or
define packaging, installation, or trust policy.

## Migration

Consumers no longer enable `ffi-prototype`; `make test` and `make check`
exercise the prototype automatically.
