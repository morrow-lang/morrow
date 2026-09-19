+++
schema_version = 1
id = "01M2XHZ972JPJX8Y3FNYKFYDH9"
title = "Doc tests for reliability"
date = "2026-01-27"
status = "accepted"
tags = ["testing", "docs"]
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
## Decision

I will support doc tests where examples in `@doc` comments are automatically tested.

## Context

Documentation often becomes stale because examples aren't verified. Rust's doc tests solve this by making documentation runnable and testable. This ensures examples always work and documentation stays current. It's especially valuable for AI-assisted development where examples serve as additional test cases.

## Consequences

The test runner must extract code blocks from `@doc` comments and execute them. Examples must be valid Fern code. Failed doc tests fail the build.
