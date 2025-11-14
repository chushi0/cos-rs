use core::{
    fmt::{Debug, Display, Formatter},
    num::NonZeroU64,
};

#[derive(Clone, Copy, Hash)]
#[repr(transparent)]
pub struct SyscallError(NonZeroU64);

pub type Result<T = (), E = SyscallError> = core::result::Result<T, E>;

#[derive(Debug, Clone, Copy, Hash)]
#[non_exhaustive]
#[repr(u64)]
pub enum ErrorKind {
    Success = 0,
    PermissionDenied = 1,
    OutOfMemory = 2,
    SegmentationFault = 3,
    BadArgument = 4,
    Unknown = u64::MAX,
}

impl SyscallError {
    pub const fn new(error: u64) -> Option<Self> {
        match NonZeroU64::new(error) {
            Some(val) => Some(Self(val)),
            None => None,
        }
    }

    pub const fn to_result(error: u64) -> Result<(), Self> {
        match NonZeroU64::new(error) {
            Some(val) => Err(Self(val)),
            None => Ok(()),
        }
    }

    pub const fn error_code(self) -> u64 {
        self.0.get()
    }

    pub const fn kind(&self) -> ErrorKind {
        ErrorKind::from_error_code(self.0.get())
    }
}

impl ErrorKind {
    pub const fn from_error_code(error_code: u64) -> Self {
        macro_rules! match_arms {
            ($error_code:ident, $($name:ident),* $(,)?) => {
                match $error_code {
                    $(
                        val if val == Self::$name as u64 => Self::$name,
                    )*
                    _ => Self::Unknown,
                }
            };
        }

        match_arms!(
            error_code,
            Success,
            PermissionDenied,
            OutOfMemory,
            SegmentationFault,
            BadArgument,
        )
    }
}

impl Debug for SyscallError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let kind = self.kind();
        if matches!(kind, ErrorKind::Unknown) {
            write!(f, "SyscallError(Unknown(0x{:x}))", self.0)
        } else {
            write!(f, "SyscallError({:?})", kind)
        }
    }
}

impl Display for SyscallError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        <ErrorKind as Display>::fmt(&self.kind(), f)
    }
}

impl Display for ErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let str = match self {
            ErrorKind::Success => "the operation is done successful",
            ErrorKind::PermissionDenied => "you do not have permission to do this",
            ErrorKind::OutOfMemory => "system is run out of memory",
            ErrorKind::SegmentationFault => "application memory broken",
            ErrorKind::BadArgument => "bad argument",
            ErrorKind::Unknown => "unknown error",
        };

        write!(f, "{}", str)
    }
}
