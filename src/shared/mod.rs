//! Cross-feature shared parts for `tuif` (SPEC-0200-0001).

pub mod abs_path;
pub mod cancel;
pub mod e2e_scope;
pub(crate) mod identity;
pub mod schema;

#[cfg(test)]
mod abs_path_tests;
#[cfg(test)]
mod cancel_tests;
#[cfg(test)]
mod e2e_scope_tests;
#[cfg(test)]
mod identity_tests;
