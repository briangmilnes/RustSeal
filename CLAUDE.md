# RustSeal

project-tz: America/Los_Angeles

 RustSeal collects the top ~1000 Rust GitHub crates as a source-only corpus
(in `RustProjects/`, git-ignored) plus the fixes and analysis tools for the
rust leak-amplification exceptionally bad coding. The analysis tools are built
using the ~/projects/CSTs code to do concrete syntax checking for Rust
programs. Many of the collected repositories are multi-crate, hence the
directory name `RustProjects/` rather than a single-crate name.

Agent rules for this project are imported below: the ComputAItionalThinking
ruleset (Personas, Language rules, and the working principles) and the GRASE
process rules. Any rules specific to RustSeal go above the imports.

@~/projects/ComputAItionalThinking/ComputAItionalThinkingRules.md
@~/projects/GRASE/GRASERules.md
