#![no_std]

pub trait AsyncRead {
    type ReadError;

    fn read(&mut self, buf: &mut [u8])
    -> impl Future<Output = Result<u64, Self::ReadError>> + Send;
}

pub trait Seekable {
    type SeekError;

    fn seek(&mut self, cursor: u64) -> impl Future<Output = Result<(), Self::SeekError>> + Send;
}

pub enum ReadExactError<E> {
    InnerError(E),
    EOF,
}

pub trait AsyncReadExt: AsyncRead {
    fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> impl Future<Output = Result<(), ReadExactError<Self::ReadError>>> + Send;
}

impl<E> From<E> for ReadExactError<E> {
    fn from(value: E) -> Self {
        Self::InnerError(value)
    }
}

impl<T> AsyncReadExt for T
where
    T: AsyncRead + Send,
{
    async fn read_exact(
        &mut self,
        mut buf: &mut [u8],
    ) -> Result<(), ReadExactError<Self::ReadError>> {
        while !buf.is_empty() {
            let c = self.read(buf).await?;
            if c == 0 {
                return Err(ReadExactError::EOF);
            }
            buf = &mut buf[c as usize..];
        }
        Ok(())
    }
}
