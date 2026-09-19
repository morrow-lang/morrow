+++
schema_version = 1
id = "01M2XHZ8T0WARFFX8TRX9SE88H"
title = "Make directory listing failures explicit in the source API"
date = "2026-09-05"
status = "accepted"
tags = []
supersedes = []
superseded_by = []
depends_on = []
related_to = []
+++
* **Status**: Accepted for the unreleased frontend migration
* **Decision**: I will change `fs.list_dir` and its `File.list_dir` alias from `List(String)` to `Result(List(String), Int)` in both frontends. Empty directories return `Ok([])`; filesystem failures return an error, never an empty or partial success.
* **Context**: The legacy native helper returned NULL for open failures even though the source type promised a List. This prevented safe Rust lowering and could cause invalid pointer access. The user authorized completing the pre-1.0 migration, including the implementation work needed to make failure handling reliable. The unavailable `/decision` skill is replaced by this established decision format.
* **Consequences**: Callers must match, propagate, or otherwise handle the Result. Error codes distinguish missing paths, permission failures, non-directories, and other IO failures. Enumeration is bounded to 1,048,576 entries; exceeding that limit returns IO failure. The legacy nullable `fern_list_dir` C ABI remains, while source calls use `fern_read_dir_result`; Rust copies successful native StringLists into ordinary Lists. This is a breaking source change for the unreleased migration and must be announced with migration guidance; it must not be published as a backward-compatible patch or minor release under the compatibility policy.
