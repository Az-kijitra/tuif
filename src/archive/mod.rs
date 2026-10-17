//! `archive/` — ZIP create / extract logic for `cuifiler`.

pub mod central_dir;
pub mod create;
pub mod entry_name;
pub mod eocd;
pub mod error;
pub mod extract;
pub mod extract_plan;
pub mod guarded;
pub mod path_rules;
pub mod progress;
pub mod temp;
pub mod win_fs;
pub mod zip_time;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod zip_fixture;
#[cfg(test)]
mod zip_fixture_tests;
#[cfg(test)]
mod error_tests;
#[cfg(test)]
mod path_rules_tests;
#[cfg(test)]
mod entry_name_tests;
#[cfg(test)]
mod eocd_tests;
#[cfg(test)]
mod temp_tests;
#[cfg(test)]
mod win_fs_tests;
#[cfg(test)]
mod progress_tests;
#[cfg(test)]
mod extract_plan_tests;
#[cfg(test)]
mod extract_tests;
#[cfg(test)]
mod create_tests;
#[cfg(test)]
mod cp932_experiment_tests;
#[cfg(test)]
mod guarded_tests;
#[cfg(test)]
mod input_resolution_tests;
