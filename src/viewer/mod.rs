//! `viewer/` — Text/Markdown classification, reading, and sibling navigation for `tuif`.

pub mod classify;
pub mod error;
pub mod highlight;
pub mod siblings;
pub mod text;

#[cfg(test)]
mod classify_tests;
#[cfg(test)]
mod error_tests;
#[cfg(test)]
mod siblings_tests;
#[cfg(test)]
mod text_tests;
