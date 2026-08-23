//! Git-native project templates.
//!
//! The rendered state of a template is a Git ref, `refs/tpl/<template-id>`.
//! Updating the template advances that ref; the user incorporates the changes
//! with a normal `git merge`. git-tpl produces the desired rendered Git state
//! and nothing else — Git owns merging, conflicts, remotes and history.
//!
//! See `docs/adr/001-rendered-ref-model.md` for the reasoning.
//!
//! # Layering
//!
//! ```text
//! ops        orchestration, one function per command
//!  ├── render      the tree walk
//!  ├── eval        prompting and expression evaluation
//!  │    └── graph  the dependency DAG
//!  ├── data        data source loading
//!  └── git         the Git abstraction (git2 lives only in git::libgit2)
//! ```
//!
//! Dependencies point inward. Nothing below `ops` knows a command exists.
//! `commands` — in the binary, not here — is the one module per subcommand.

#![warn(missing_docs)]
#![warn(clippy::all)]
// Several error enums carry a `NamedSource<String>` for miette diagnostics,
// which makes them large. Boxing them would obscure the diagnostic derive for
// no benefit: these are returned on paths that are about to terminate the
// program or print an error, never in a hot loop.
#![allow(clippy::result_large_err)]

pub mod answers;
pub mod config;
pub mod context;
pub mod data;
pub mod eval;
pub mod git;
pub mod gitconfig;
pub mod graph;
pub mod lint;
pub mod migration;
pub mod note;
pub mod ops;
pub mod provenance;
pub mod refs;
pub mod remote;
pub mod render;
pub mod seed;
pub mod suggest;
pub mod template;
pub mod userconfig;

pub use config::{Config, ConfigError};
pub use context::Context;
pub use data::Loader;
pub use graph::Graph;
pub use refs::TemplateId;
pub use template::{Manifest, Value};
pub use userconfig::{UserConfig, UserConfigError};

/// The version of git-tpl, recorded in the provenance trailers of every
/// rendered commit so that a rendering difference caused by an engine change is
/// attributable.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
