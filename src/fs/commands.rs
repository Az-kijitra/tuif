//! Directory and drive enumeration wrapper around `fs::query` and `fs::win_drives`.

use std::io;
use std::path::PathBuf;

use serde::Serialize;

use super::query::{self, EntryType};
use crate::shared::abs_path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsListDirErrorDto {
    pub code: String,
    pub message: String,
}

impl From<io::Error> for FsListDirErrorDto {
    fn from(e: io::Error) -> Self {
        let code = match e.kind() {
            io::ErrorKind::PermissionDenied => "access_denied",
            io::ErrorKind::NotFound => "not_found",
            io::ErrorKind::NotADirectory => "not_a_directory",
            _ => "other",
        };
        Self {
            code: code.to_string(),
            message: e.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsEntryDto {
    pub name: String,
    pub path: String,
    pub entry_type: String,
    pub size: u64,
    pub modified: String,
    pub hidden: bool,
    pub ext: String,
}

impl From<query::Entry> for FsEntryDto {
    fn from(entry: query::Entry) -> Self {
        FsEntryDto {
            name: entry.name,
            path: entry.path.to_string_lossy().into_owned(),
            entry_type: match entry.entry_type {
                EntryType::File => "file".to_string(),
                EntryType::Dir => "dir".to_string(),
            },
            size: entry.size,
            modified: entry.modified,
            hidden: entry.hidden,
            ext: entry.ext,
        }
    }
}

pub fn fs_list_dir(
    path: String,
    show_hidden: bool,
    sort_key: String,
    sort_order: String,
) -> Result<Vec<FsEntryDto>, FsListDirErrorDto> {
    list_dir_checked(&path, show_hidden, &sort_key, &sort_order)
}

impl FsListDirErrorDto {
    pub fn invalid_path(path: &str) -> Self {
        Self {
            code: abs_path::INVALID_PATH_CODE.to_string(),
            message: format!("not an absolute path: {path}"),
        }
    }
}

pub(crate) fn list_dir_checked(
    path: &str,
    show_hidden: bool,
    sort_key: &str,
    sort_order: &str,
) -> Result<Vec<FsEntryDto>, FsListDirErrorDto> {
    if abs_path::first_relative([path]).is_some() {
        return Err(FsListDirErrorDto::invalid_path(path));
    }
    query::fs_list_dir(&PathBuf::from(path), show_hidden, sort_key, sort_order)
        .map(|entries| entries.into_iter().map(FsEntryDto::from).collect())
        .map_err(FsListDirErrorDto::from)
}

pub fn fs_list_drives() -> Vec<String> {
    super::win_drives::list_logical_drives()
}
