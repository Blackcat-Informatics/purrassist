// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
#![allow(unsafe_code)]

//! Windows-only durable replace/rename and directory-flush primitives.
//!
//! # Scope of the `unsafe_code = "deny"` exception
//! The workspace denies `unsafe_code` everywhere else; this module is a
//! registered exception (`#![allow(unsafe_code)]` above, scoped to this file
//! only — `crates/host/src/files.rs` and the rest of `purrassist-host` remain
//! covered by the workspace-wide deny). It exists to call `kernel32`
//! functions through `windows-sys` that have no safe std equivalent:
//! `SetFileInformationByHandle` (POSIX-semantics rename via an open handle),
//! `MoveFileExW` (write-through rename fallback), and `FlushFileBuffers` (
//! directory metadata flush). See `docs/DEPENDENCIES.md` and `SECURITY.md`
//! for the registered rationale.
//!
//! # Design
//! [`replace`] first tries `SetFileInformationByHandle` with
//! `FileRenameInfoEx` and `FILE_RENAME_FLAG_POSIX_SEMANTICS |
//! FILE_RENAME_FLAG_REPLACE_IF_EXISTS` on the already-open, write-through
//! temporary-file handle. If the filesystem rejects POSIX semantics
//! (`ERROR_INVALID_PARAMETER` or `ERROR_NOT_SUPPORTED`), it closes that
//! handle — a rename-by-path can otherwise fail with a sharing violation
//! against our own open handle — and falls back to `MoveFileExW` with
//! `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`. Either attempt that
//! instead fails with `ERROR_SHARING_VIOLATION` or `ERROR_LOCK_VIOLATION`
//! (antivirus scanners and search indexers commonly hold a brief handle open)
//! is retried [`RENAME_ATTEMPTS`] times, yielding the calling thread between
//! attempts via [`std::thread::yield_now`] — never sleeping — before giving up
//! with an [`Error::Io`] carrying the last operating-system error. Any other
//! failure, including `ERROR_ACCESS_DENIED`, is reported immediately.
//!
//! Renaming through a handle requires `DELETE` access on that handle, so the
//! temporary file is opened with `GENERIC_WRITE | DELETE` (see
//! [`prepare_temporary_options`]); plain `OpenOptions::write` does not grant it.
//!
//! [`sync_directory`] opens the directory with `FILE_FLAG_BACKUP_SEMANTICS`
//! and calls `FlushFileBuffers` on it. Some filesystems deny this
//! (`ERROR_ACCESS_DENIED`, at open or at flush); that is reported as
//! [`DirectorySync::AccessDeniedFallback`] rather than treated as an error,
//! since the write-through rename already made the file replacement durable
//! on its own — the directory flush is belt-and-suspenders, not the sole
//! durability boundary.
use super::{DirectorySync, Error, RenameMethod};
use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    fs::File,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_LOCK_VIOLATION, ERROR_NOT_SUPPORTED,
        ERROR_SHARING_VIOLATION, GENERIC_WRITE, GetLastError, HANDLE,
    },
    Storage::FileSystem::{
        DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_WRITE_THROUGH, FILE_RENAME_INFO,
        FileRenameInfoEx, FlushFileBuffers, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        MoveFileExW, SetFileInformationByHandle,
    },
    System::WindowsProgramming::{
        FILE_RENAME_FLAG_POSIX_SEMANTICS, FILE_RENAME_FLAG_REPLACE_IF_EXISTS,
    },
};

/// Fixed bounded-retry count for a rename attempt that fails with a
/// transient sharing/lock violation. No attempt sleeps the calling thread;
/// each retry only yields it, so the bound is on attempt count, not
/// wall-clock time.
const RENAME_ATTEMPTS: u32 = 5;

/// Configure the temporary file's create options: write-through, so every
/// accepted write bypasses the cache and is durable as soon as `write_all`
/// returns, and `GENERIC_WRITE | DELETE` access, because
/// `SetFileInformationByHandle` renames through this handle and Windows
/// requires `DELETE` access for that.
pub(super) fn prepare_temporary_options(options: &mut std::fs::OpenOptions) {
    options
        .access_mode(GENERIC_WRITE | DELETE)
        .custom_flags(FILE_FLAG_WRITE_THROUGH);
}

/// Replace `target` with the bytes already written to `temporary` through
/// `file`, preferring POSIX rename semantics and falling back to
/// `MoveFileExW` where the filesystem rejects them. Returns which mechanism
/// actually completed the rename, as durability evidence for the caller.
pub(super) fn replace(file: File, temporary: &Path, target: &Path) -> Result<RenameMethod, Error> {
    let target_name: Vec<u16> = target.as_os_str().encode_wide().collect();
    let flags = FILE_RENAME_FLAG_POSIX_SEMANTICS | FILE_RENAME_FLAG_REPLACE_IF_EXISTS;
    let buffer = RenameBuffer::new(&target_name, flags)?;
    match retry(|| set_rename_info(&file, &buffer), is_posix_unsupported) {
        Outcome::Success => return Ok(RenameMethod::PosixSemantics),
        Outcome::Contended(code) | Outcome::Io(code) => return Err(win_io_error(code)),
        Outcome::Unsupported => {}
    }
    // The filesystem rejected POSIX rename semantics. Close our own handle
    // first: `MoveFileExW` renames by path, and an open handle without
    // `FILE_SHARE_DELETE` can otherwise turn this into a sharing violation
    // against ourselves.
    drop(file);
    let existing: Vec<u16> = wide_null(temporary);
    let destination: Vec<u16> = wide_null(target);
    match retry(|| move_file_ex(&existing, &destination), |_| false) {
        Outcome::Success => Ok(RenameMethod::MoveFileExFallback),
        Outcome::Contended(code) | Outcome::Io(code) => Err(win_io_error(code)),
        Outcome::Unsupported => unreachable!("MoveFileExW fallback never reports Unsupported"),
    }
}

/// Flush a directory's metadata to durable storage, reporting rather than
/// failing when the platform denies it.
pub(super) fn sync_directory(path: &Path) -> Result<DirectorySync, Error> {
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if is_access_denied(&error) => return Ok(DirectorySync::AccessDeniedFallback),
        Err(error) => return Err(Error::Io(error)),
    };
    // SAFETY: `file` owns a valid, currently-open directory HANDLE for the
    // duration of this call; `FlushFileBuffers` only reads that handle.
    let ok = unsafe { FlushFileBuffers(file.as_raw_handle() as HANDLE) };
    if ok != 0 {
        return Ok(DirectorySync::Flushed);
    }
    // SAFETY: queries the error for the call immediately above, same thread.
    let code = unsafe { GetLastError() };
    if code == ERROR_ACCESS_DENIED {
        Ok(DirectorySync::AccessDeniedFallback)
    } else {
        Err(win_io_error(code))
    }
}

fn is_access_denied(error: &std::io::Error) -> bool {
    error.raw_os_error() == i32::try_from(ERROR_ACCESS_DENIED).ok()
}

enum Outcome {
    Success,
    /// Still sharing- or lock-violated after every bounded attempt; carries
    /// the last operating-system error code.
    Contended(u32),
    Io(u32),
    Unsupported,
}

/// Run `attempt` up to [`RENAME_ATTEMPTS`] times. `is_unsupported` classifies
/// a capability rejection (no retry: the caller should fall back);
/// `ERROR_SHARING_VIOLATION`/`ERROR_LOCK_VIOLATION` are treated as transient
/// and retried after yielding the thread; any other code is reported
/// immediately as an I/O error.
fn retry(
    mut attempt: impl FnMut() -> Result<(), u32>,
    is_unsupported: impl Fn(u32) -> bool,
) -> Outcome {
    for remaining in (0..RENAME_ATTEMPTS).rev() {
        match attempt() {
            Ok(()) => return Outcome::Success,
            Err(code) if is_unsupported(code) => return Outcome::Unsupported,
            Err(code) if is_sharing_violation(code) => {
                if remaining == 0 {
                    return Outcome::Contended(code);
                }
                std::thread::yield_now();
            }
            Err(code) => return Outcome::Io(code),
        }
    }
    Outcome::Contended(ERROR_SHARING_VIOLATION)
}

const fn is_posix_unsupported(code: u32) -> bool {
    code == ERROR_INVALID_PARAMETER || code == ERROR_NOT_SUPPORTED
}

const fn is_sharing_violation(code: u32) -> bool {
    matches!(code, ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
}

fn win_io_error(code: u32) -> Error {
    Error::Io(std::io::Error::from_raw_os_error(
        i32::try_from(code).unwrap_or(i32::MAX),
    ))
}

fn wide_null(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn set_rename_info(file: &File, buffer: &RenameBuffer) -> Result<(), u32> {
    // SAFETY: `file` owns a currently-open HANDLE for the duration of this
    // call. `buffer` points to a `FILE_RENAME_INFO` whose trailing `FileName`
    // region was sized and initialized in `RenameBuffer::new` to exactly
    // `buffer.len` bytes, matching `dwbuffersize`.
    let ok = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle() as HANDLE,
            FileRenameInfoEx,
            buffer.ptr.cast(),
            buffer.len,
        )
    };
    if ok != 0 {
        Ok(())
    } else {
        // SAFETY: queries the error for the call immediately above, same thread.
        Err(unsafe { GetLastError() })
    }
}

fn move_file_ex(existing: &[u16], new: &[u16]) -> Result<(), u32> {
    // SAFETY: both slices are NUL-terminated UTF-16 buffers kept alive for
    // the duration of this call.
    let ok = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            new.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok != 0 {
        Ok(())
    } else {
        // SAFETY: queries the error for the call immediately above, same thread.
        Err(unsafe { GetLastError() })
    }
}

/// An owned, correctly aligned `FILE_RENAME_INFO` followed by its variable-length
/// UTF-16 file name, sized for exactly one name and freed on drop.
struct RenameBuffer {
    ptr: *mut u8,
    layout: Layout,
    len: u32,
}
impl RenameBuffer {
    fn new(name: &[u16], flags: u32) -> Result<Self, Error> {
        let header = std::mem::size_of::<FILE_RENAME_INFO>();
        let name_bytes = name.len().saturating_mul(std::mem::size_of::<u16>());
        let file_name_length = u32::try_from(name_bytes)
            .map_err(|_| Error::Invalid("rename target path too long".into()))?;
        let declared = std::mem::size_of::<u16>(); // `FileName: [u16; 1]` already reserves this much.
        let extra = name_bytes.saturating_sub(declared);
        let size = header
            .checked_add(extra)
            .ok_or_else(|| Error::Invalid("rename target path too long".into()))?;
        let len = u32::try_from(size)
            .map_err(|_| Error::Invalid("rename target path too long".into()))?;
        let layout = Layout::from_size_align(size, std::mem::align_of::<FILE_RENAME_INFO>())
            .map_err(|_| Error::Invalid("rename target path too long".into()))?;
        // SAFETY: `layout` has a non-zero size (`FILE_RENAME_INFO` alone is
        // non-zero-sized); allocation failure is checked immediately below.
        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            return Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::OutOfMemory,
            )));
        }
        // SAFETY: `ptr` was just allocated by `alloc_zeroed` with exactly
        // enough room for one `FILE_RENAME_INFO` followed by `name`'s UTF-16
        // units; every field written is primitive/POD, this is the only
        // live reference to `ptr`, and `FileName`'s declared one-element
        // array is the documented Win32 flexible-array-member convention
        // for this struct.
        unsafe {
            // `layout` uses `align_of::<FILE_RENAME_INFO>()`, so the byte
            // pointer returned by `alloc_zeroed` is suitably aligned for it.
            #[allow(clippy::cast_ptr_alignment)]
            let info = ptr.cast::<FILE_RENAME_INFO>();
            (*info).Anonymous.Flags = flags;
            (*info).RootDirectory = std::ptr::null_mut();
            (*info).FileNameLength = file_name_length;
            std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        }
        Ok(Self { ptr, layout, len })
    }
}
impl Drop for RenameBuffer {
    fn drop(&mut self) {
        // SAFETY: `ptr`/`layout` are exactly as returned from `alloc_zeroed`
        // in `RenameBuffer::new`, and a `RenameBuffer` is dropped at most once.
        unsafe { dealloc(self.ptr, self.layout) };
    }
}
