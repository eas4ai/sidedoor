//! Plugin discovery, wire protocol, and the shared Bun supervisor.
pub mod discovery;
pub mod install;
pub mod manifest;
pub mod protocol;
pub mod reload;
pub mod runtime;
pub mod sdk;
pub use discovery::*;
pub use install::{GitHub, GitHubInstaller, Installer, Source, Staged};
pub use manifest::*;
pub use protocol::*;
pub use reload::fingerprint;
pub use runtime::Runner;
pub use sdk::{TSCONFIG, create, find_bun, sdk_dir};
#[cfg(test)]
mod tests;
