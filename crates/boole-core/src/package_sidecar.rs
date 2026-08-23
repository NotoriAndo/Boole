//! BF.6a — canonical useful-work package bytes and a distinct sidecar root.
//!
//! Binary layout (all integer prefixes are big-endian):
//!
//! ```text
//! u32 schema_len || schema_utf8 || u32 file_count ||
//!   repeated(sorted by path UTF-8 bytes) {
//!     u32 path_len || path_utf8 || u64 content_len || content_bytes
//!   }
//! ```

use thiserror::Error;

use crate::hash::{h_protocol, Hex32, Hex32Error};

pub const PACKAGE_SIDECAR_SCHEMA: &str = "boole.useful-work.package-sidecar.v1";
pub const PACKAGE_SIDECAR_ROOT_DOMAIN: &[u8] = b"boole.useful-work.package-sidecar.v1";
pub const MAX_PACKAGE_FILES: usize = 4096;
pub const MAX_PACKAGE_CANONICAL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFile {
    path: Vec<u8>,
    contents: Vec<u8>,
}

impl PackageFile {
    pub fn new(path: impl AsRef<[u8]>, contents: impl AsRef<[u8]>) -> Self {
        Self {
            path: path.as_ref().to_vec(),
            contents: contents.as_ref().to_vec(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageRoot(Hex32);

impl PackageRoot {
    pub fn from_hex(value: &str) -> Result<Self, Hex32Error> {
        Hex32::from_hex(value).map(Self)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PackageSidecarError {
    #[error("package path must be UTF-8")]
    PathNotUtf8,
    #[error("package path must not be empty")]
    EmptyPath,
    #[error("package path must not contain empty components")]
    EmptyPathComponent,
    #[error("package path must be relative")]
    AbsolutePath,
    #[error("package path must not contain `.` or `..` components")]
    DotPathComponent,
    #[error("package path must use `/`, never `\\`")]
    BackslashInPath,
    #[error("package path must not contain NUL")]
    NulInPath,
    #[error("package contains a duplicate path")]
    DuplicatePath,
    #[error("package has {count} files; maximum is {max}")]
    TooManyFiles { count: usize, max: usize },
    #[error("canonical package is {size} bytes; maximum is {max}")]
    PackageTooLarge { size: usize, max: usize },
    #[error("canonical package bytes are truncated or have an invalid length")]
    MalformedCanonicalBytes,
    #[error("canonical package schema does not match the frozen sidecar schema")]
    SchemaMismatch,
    #[error("canonical package bytes are not in the unique canonical encoding")]
    NonCanonicalEncoding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPackage {
    canonical_bytes: Vec<u8>,
    root: PackageRoot,
}

impl CanonicalPackage {
    pub fn new(mut files: Vec<PackageFile>) -> Result<Self, PackageSidecarError> {
        if files.len() > MAX_PACKAGE_FILES {
            return Err(PackageSidecarError::TooManyFiles {
                count: files.len(),
                max: MAX_PACKAGE_FILES,
            });
        }
        for file in &files {
            validate_path(&file.path)?;
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        if files.windows(2).any(|pair| pair[0].path == pair[1].path) {
            return Err(PackageSidecarError::DuplicatePath);
        }

        let canonical_size = canonical_size(&files)?;
        let mut canonical_bytes = Vec::with_capacity(canonical_size);
        canonical_bytes.extend_from_slice(&(PACKAGE_SIDECAR_SCHEMA.len() as u32).to_be_bytes());
        canonical_bytes.extend_from_slice(PACKAGE_SIDECAR_SCHEMA.as_bytes());
        canonical_bytes.extend_from_slice(&(files.len() as u32).to_be_bytes());
        for file in files {
            canonical_bytes.extend_from_slice(&(file.path.len() as u32).to_be_bytes());
            canonical_bytes.extend_from_slice(&file.path);
            canonical_bytes.extend_from_slice(&(file.contents.len() as u64).to_be_bytes());
            canonical_bytes.extend_from_slice(&file.contents);
        }
        let root = PackageRoot(h_protocol(PACKAGE_SIDECAR_ROOT_DOMAIN, &[&canonical_bytes]));

        Ok(Self {
            canonical_bytes,
            root,
        })
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, PackageSidecarError> {
        if bytes.len() > MAX_PACKAGE_CANONICAL_BYTES {
            return Err(PackageSidecarError::PackageTooLarge {
                size: bytes.len(),
                max: MAX_PACKAGE_CANONICAL_BYTES,
            });
        }
        let mut cursor = 0usize;
        let schema_len = read_u32(bytes, &mut cursor)? as usize;
        let schema = take_bytes(bytes, &mut cursor, schema_len)?;
        if schema != PACKAGE_SIDECAR_SCHEMA.as_bytes() {
            return Err(PackageSidecarError::SchemaMismatch);
        }
        let file_count = read_u32(bytes, &mut cursor)? as usize;
        if file_count > MAX_PACKAGE_FILES {
            return Err(PackageSidecarError::TooManyFiles {
                count: file_count,
                max: MAX_PACKAGE_FILES,
            });
        }
        let mut files = Vec::with_capacity(file_count);
        for _ in 0..file_count {
            let path_len = read_u32(bytes, &mut cursor)? as usize;
            let path = take_bytes(bytes, &mut cursor, path_len)?;
            let content_len = usize::try_from(read_u64(bytes, &mut cursor)?)
                .map_err(|_| PackageSidecarError::MalformedCanonicalBytes)?;
            let contents = take_bytes(bytes, &mut cursor, content_len)?;
            files.push(PackageFile::new(path, contents));
        }
        if cursor != bytes.len() {
            return Err(PackageSidecarError::NonCanonicalEncoding);
        }
        let package = Self::new(files)?;
        if package.canonical_bytes != bytes {
            return Err(PackageSidecarError::NonCanonicalEncoding);
        }
        Ok(package)
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub fn size_bytes(&self) -> usize {
        self.canonical_bytes.len()
    }

    pub fn root(&self) -> PackageRoot {
        self.root
    }
}

fn take_bytes<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], PackageSidecarError> {
    let end = cursor
        .checked_add(length)
        .ok_or(PackageSidecarError::MalformedCanonicalBytes)?;
    let value = bytes
        .get(*cursor..end)
        .ok_or(PackageSidecarError::MalformedCanonicalBytes)?;
    *cursor = end;
    Ok(value)
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, PackageSidecarError> {
    let encoded: [u8; 4] = take_bytes(bytes, cursor, 4)?
        .try_into()
        .map_err(|_| PackageSidecarError::MalformedCanonicalBytes)?;
    Ok(u32::from_be_bytes(encoded))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, PackageSidecarError> {
    let encoded: [u8; 8] = take_bytes(bytes, cursor, 8)?
        .try_into()
        .map_err(|_| PackageSidecarError::MalformedCanonicalBytes)?;
    Ok(u64::from_be_bytes(encoded))
}

fn canonical_size(files: &[PackageFile]) -> Result<usize, PackageSidecarError> {
    let mut size = 4usize
        .checked_add(PACKAGE_SIDECAR_SCHEMA.len())
        .and_then(|value| value.checked_add(4))
        .ok_or(PackageSidecarError::PackageTooLarge {
            size: usize::MAX,
            max: MAX_PACKAGE_CANONICAL_BYTES,
        })?;
    for file in files {
        size = size
            .checked_add(4)
            .and_then(|value| value.checked_add(file.path.len()))
            .and_then(|value| value.checked_add(8))
            .and_then(|value| value.checked_add(file.contents.len()))
            .ok_or(PackageSidecarError::PackageTooLarge {
                size: usize::MAX,
                max: MAX_PACKAGE_CANONICAL_BYTES,
            })?;
    }
    if size > MAX_PACKAGE_CANONICAL_BYTES {
        return Err(PackageSidecarError::PackageTooLarge {
            size,
            max: MAX_PACKAGE_CANONICAL_BYTES,
        });
    }
    Ok(size)
}

fn validate_path(path: &[u8]) -> Result<(), PackageSidecarError> {
    std::str::from_utf8(path).map_err(|_| PackageSidecarError::PathNotUtf8)?;
    if path.is_empty() {
        return Err(PackageSidecarError::EmptyPath);
    }
    if path.contains(&0) {
        return Err(PackageSidecarError::NulInPath);
    }
    if path.contains(&b'\\') {
        return Err(PackageSidecarError::BackslashInPath);
    }
    let windows_drive_path = path.len() >= 2 && path[0].is_ascii_alphabetic() && path[1] == b':';
    if path.starts_with(b"/") || windows_drive_path {
        return Err(PackageSidecarError::AbsolutePath);
    }
    for component in path.split(|byte| *byte == b'/') {
        if component.is_empty() {
            return Err(PackageSidecarError::EmptyPathComponent);
        }
        if component == b"." || component == b".." {
            return Err(PackageSidecarError::DotPathComponent);
        }
    }
    Ok(())
}
