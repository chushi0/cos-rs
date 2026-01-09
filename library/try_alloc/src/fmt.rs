use core::fmt::{Arguments, Error as FmtError, Write};

use crate::error::AllocError;

#[macro_export]
macro_rules! try_write {
    ($w:expr, $($arg:tt)*) => {
        $crate::fmt::TryWrite::try_write_fmt($w, format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! try_writeln {
    ($w:expr) => {
        $crate::try_write!($w, "\n");
    };
    ($w:expr, $($arg:tt)*) => {
        $crate::try_write!($w, "{}\n", format_args!($($arg)*));
    };
}

pub trait TryWrite {
    fn try_write_str(&mut self, s: &str) -> Result<(), TryWriteError>;

    fn try_write_char(&mut self, c: char) -> Result<(), TryWriteError> {
        self.try_write_str(c.encode_utf8(&mut [0; 4]))
    }

    fn try_write_fmt(&mut self, args: Arguments<'_>) -> Result<(), TryWriteError>
    where
        Self: Sized,
    {
        let mut w = Write2TryWrite::new(self as &mut dyn TryWrite);
        w.write_fmt(args).map_err(|e| w.map_try_error(e))?;
        Ok(())
    }
}

pub enum TryWriteError {
    FmtError(FmtError),
    AllocError(AllocError),
}

pub(crate) struct Write2TryWrite<'w> {
    inner_write: &'w mut dyn TryWrite,
    alloc_error: bool,
}

impl<'w> Write2TryWrite<'w> {
    pub fn new(write: &'w mut dyn TryWrite) -> Self {
        Self {
            inner_write: write,
            alloc_error: false,
        }
    }

    pub fn map_try_error(&self, e: FmtError) -> TryWriteError {
        if self.alloc_error {
            TryWriteError::AllocError(AllocError)
        } else {
            TryWriteError::FmtError(e)
        }
    }
}

impl Write for Write2TryWrite<'_> {
    fn write_str(&mut self, s: &str) -> Result<(), FmtError> {
        if self.alloc_error {
            return Err(FmtError);
        }

        self.inner_write.try_write_str(s).map_err(|e| match e {
            TryWriteError::FmtError(error) => error,
            TryWriteError::AllocError(_) => {
                self.alloc_error = true;
                FmtError
            }
        })
    }

    fn write_char(&mut self, c: char) -> Result<(), FmtError> {
        if self.alloc_error {
            return Err(FmtError);
        }

        self.inner_write.try_write_char(c).map_err(|e| match e {
            TryWriteError::FmtError(error) => error,
            TryWriteError::AllocError(_) => {
                self.alloc_error = true;
                FmtError
            }
        })
    }
}
