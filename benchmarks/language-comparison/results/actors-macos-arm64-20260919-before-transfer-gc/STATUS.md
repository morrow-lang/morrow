# Complete pre-transfer-GC checkpoint

This is a complete, valid 216-process matrix retained as a historical
checkpoint. It contains 36 warmups and 180 measured runs, with five measured
rounds in every workload, scheduler-count and implementation cell. Every child
passed the independent output oracle, all stderr was empty and no run timed out.

The Morrow artifact includes the root-driver progress fix documented in the
separate `actors-macos-arm64-20260919-before-fix` discovery evidence. It predates
the accepted optimization that performs a precise collection at a candidate
callback boundary before heap transfer. That optimization changes the code under
test, so this matrix was preserved rather than overwritten or mixed with the
post-change samples.

`inputs.txt` records the workload, runner, compiler, runtime archive, Elixir and
BEAM hashes. `source-manifest.sha256` binds the measurements to 313 relevant
Rust source and Cargo inputs. The original raw streams, event timestamps,
measurement CSV and derived summary are unchanged.

This directory is useful for before/after analysis of the transfer path. It is
not the final post-optimization language comparison.
