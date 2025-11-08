use core::{ffi::CStr, ops::Add, str::FromStr};

use alloc::{string::String, vec::Vec};

/// 路径分隔符
pub const DELIMITER: u8 = b'/';

/// 文件路径
///
/// 文件路径是任意合法的UTF8字符串表示。
///
/// 文件路径并不关心它是相对路径还是绝对路径——您可以随意将路径进行拼接，以实现相对路径和绝对路径的转换。
/// 但当传递给文件系统函数时，文件系统将视为绝对路径进行处理。
///
/// 文件路径的分隔符应为 [`DELIMITER`]。PathBuf的构造函数会忽略其余部分的合法性检验，程序的其余部分应负责检查路径的合法性
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct PathBuf {
    segments: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Path<'path> {
    segments: &'path [String],
}

/// 路径解析错误
#[derive(Debug)]
pub enum ParsePathError {
    InvalidSegments,
}

impl PathBuf {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ParsePathError> {
        if bytes.is_empty() {
            return Ok(Self::default());
        }

        let segments = bytes
            .split(|&b| b == DELIMITER)
            .filter(|bytes| !bytes.is_empty())
            .map(|segment| {
                String::from_utf8(segment.to_vec()).map_err(|_| ParsePathError::InvalidSegments)
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { segments })
    }

    pub fn from_cstr(s: &CStr) -> Result<Self, ParsePathError> {
        Self::from_bytes(s.to_bytes())
    }

    pub fn from_str(s: &str) -> Result<Self, ParsePathError> {
        Self::from_bytes(s.as_bytes())
    }

    pub fn extends(&mut self, other: &PathBuf) {
        self.segments.extend_from_slice(&other.segments);
    }

    pub fn as_path(&self) -> Path<'_> {
        Path {
            segments: &self.segments,
        }
    }
}

impl FromStr for PathBuf {
    type Err = ParsePathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str(s)
    }
}

impl Add<&PathBuf> for &PathBuf {
    type Output = PathBuf;

    fn add(self, rhs: &PathBuf) -> PathBuf {
        let mut segments = self.segments.clone();
        segments.extend_from_slice(&rhs.segments);
        PathBuf { segments }
    }
}

impl Path<'_> {
    pub fn iter(&self) -> PathIter<'_> {
        PathIter {
            inner: self.segments.iter(),
        }
    }

    pub fn parent(&self) -> Path<'_> {
        if self.is_root() {
            Path { segments: &[] }
        } else {
            Path {
                segments: &self.segments[..self.segments.len() - 1],
            }
        }
    }

    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn last_segment(&self) -> Option<&str> {
        self.segments.last().map(|s| s.as_str())
    }
}

pub struct PathIter<'s> {
    inner: core::slice::Iter<'s, String>,
}

impl<'s> Iterator for PathIter<'s> {
    type Item = &'s str;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|s| s.as_str())
    }
}
