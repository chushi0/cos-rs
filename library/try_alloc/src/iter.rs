use crate::error::AllocError;

pub trait TryFromIterator<A>: FromIterator<A> {
    fn try_from_iter<T: IntoIterator<Item = A>>(iter: T) -> Result<Self, AllocError>;
}

pub trait TryCollect<A> {
    fn try_collect<T: TryFromIterator<A>>(self) -> Result<T, AllocError>;
}

impl<A, I> TryCollect<A> for I
where
    I: IntoIterator<Item = A>,
{
    fn try_collect<T: TryFromIterator<A>>(self) -> Result<T, AllocError> {
        TryFromIterator::try_from_iter(self)
    }
}
