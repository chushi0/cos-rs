use core::fmt::{Display, Write};

use alloc::{
    borrow::{Cow, ToOwned},
    string::String,
};

use crate::{
    clone::TryClone,
    error::AllocError,
    fmt::{TryWrite, TryWriteError, Write2TryWrite},
};

pub trait TryString {
    fn try_with_capacity(capacity: usize) -> Result<Self, AllocError>
    where
        Self: Sized;

    fn try_push(&mut self, ch: char) -> Result<(), AllocError>;

    fn try_push_str(&mut self, string: &str) -> Result<(), AllocError>;

    fn try_from_utf8_lossy(v: &[u8]) -> Result<Cow<'_, str>, AllocError>;
}

impl TryString for String {
    fn try_with_capacity(capacity: usize) -> Result<Self, AllocError>
    where
        Self: Sized,
    {
        let mut s = Self::new();
        s.try_reserve(capacity).map_err(|_| AllocError)?;

        Ok(s)
    }

    fn try_push(&mut self, ch: char) -> Result<(), AllocError> {
        self.try_reserve(ch.len_utf8())
            .map(|_| self.push(ch))
            .map_err(|_| AllocError)
    }

    fn try_push_str(&mut self, string: &str) -> Result<(), AllocError> {
        self.try_reserve(string.len())
            .map(|_| self.push_str(string))
            .map_err(|_| AllocError)
    }

    fn try_from_utf8_lossy(v: &[u8]) -> Result<Cow<'_, str>, AllocError> {
        let mut iter = v.utf8_chunks();

        let first_valid = if let Some(chunk) = iter.next() {
            let valid = chunk.valid();
            if chunk.invalid().is_empty() {
                debug_assert_eq!(valid.len(), v.len());
                return Ok(Cow::Borrowed(valid));
            }
            valid
        } else {
            return Ok(Cow::Borrowed(""));
        };

        const REPLACEMENT: &str = "\u{FFFD}";

        let mut res = <Self as TryString>::try_with_capacity(v.len())?;
        res.try_push_str(first_valid)?;
        res.try_push_str(REPLACEMENT)?;

        for chunk in iter {
            res.try_push_str(chunk.valid())?;
            if !chunk.invalid().is_empty() {
                res.try_push_str(REPLACEMENT)?;
            }
        }

        Ok(Cow::Owned(res))
    }
}

impl TryWrite for String {
    fn try_write_str(&mut self, s: &str) -> Result<(), TryWriteError> {
        self.try_push_str(s).map_err(TryWriteError::AllocError)
    }

    fn try_write_char(&mut self, c: char) -> Result<(), TryWriteError> {
        self.try_push(c).map_err(TryWriteError::AllocError)
    }
}

impl TryClone for String {
    fn try_clone(&self) -> Result<Self, AllocError> {
        try_clone_str_to_string(self)
    }

    fn try_clone_from(self, source: &Self) -> Result<Self, AllocError> {
        try_clone_str_into_string(self, source)
    }
}

pub(crate) fn try_clone_str_to_string(source: &str) -> Result<String, AllocError> {
    let mut string = <String as TryString>::try_with_capacity(source.len())?;
    string.push_str(source);
    Ok(string)
}

pub(crate) fn try_clone_str_into_string(
    mut string: String,
    source: &str,
) -> Result<String, AllocError> {
    if source.len() > string.len() {
        string
            .try_reserve(source.len() - string.len())
            .map_err(|_| AllocError)?;
    }

    // 由于我们已经预留了足够的空间，clone_into不会触发分配
    source.clone_into(&mut string);

    Ok(string)
}

pub trait TryToString {
    fn try_to_string(&self) -> Result<String, AllocError>;
}

impl<T> TryToString for T
where
    T: Display,
{
    fn try_to_string(&self) -> Result<String, AllocError> {
        let mut buf = String::new();

        let mut w2tw = Write2TryWrite::new(&mut buf as &mut dyn TryWrite);

        write!(w2tw, "{}", self)
            .map_err(|e| w2tw.map_try_error(e))
            .map_err(|e| match e {
                TryWriteError::FmtError(_) => {
                    panic!("a Display implementation returned an error unexpectedly")
                }
                TryWriteError::AllocError(alloc_error) => alloc_error,
            })
            .map(|_| buf)
    }
}
