//! `fs_ops/` — copy / move / delete / rename / create logic for `cuifiler`.

pub mod create;
pub mod delete;
pub mod error;
pub mod guarded;
pub mod name_validation;
pub mod preflight;
pub mod rename;
pub mod transfer;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod detect_conflicts_tests;
#[cfg(test)]
mod overwrite_merge_tests;
#[cfg(test)]
mod data_safety_tests;
#[cfg(test)]
mod guarded_tests;
