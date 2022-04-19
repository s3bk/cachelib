/**
Goals:
 - limit global memory use
 - minimize computation time
 
Track globally:
  - average value
  - average age

Track for each entry:
 - age: time since last use
 - count: how often it was used
 - amout of memory used
 - time required to compute the value

 use = count / (age)
 value = time / memory

Don't add:
 - values lower than average

Don't evict:
 - newer values
**/

use std::sync::{Arc};

#[cfg(feature="sync")]
pub mod sync;

#[cfg(feature="async")]
pub mod r#async;

#[cfg(feature="global")]
pub mod global;

#[cfg(not(feature="global"))]
pub mod global {
    use std::sync::Weak;
    use super::CacheControl;

    pub struct GlobalCache;
    impl GlobalCache {
        pub fn register(_: Weak<impl CacheControl>) {}
    }
}

pub trait CacheControl: Sync + Send + 'static {
    fn name(&self) -> Option<&str>;
    fn clean(&self, threshold: f32) -> (usize, f32);
}
pub trait ValueSize {
    fn size(&self) -> usize;
}
impl<T: ValueSize, E: ValueSize> ValueSize for Result<T, E> {
    #[inline]
    fn size(&self) -> usize {
        match self {
            &Ok(ref t) => t.size(),
            &Err(ref e) => e.size(),
        }
    }
}
impl<T: ?Sized + ValueSize> ValueSize for Arc<T> {
    #[inline]
    fn size(&self) -> usize {
        ValueSize::size(&**self)
    }
}
impl ValueSize for [u8] {
    #[inline]
    fn size(&self) -> usize {
        self.len()
    }
}
impl<T: ValueSize> ValueSize for Option<T> {
    #[inline]
    fn size(&self) -> usize {
        match *self {
            None => 0,
            Some(ref val) => val.size()
        }
    }
}
impl ValueSize for () {
    #[inline]
    fn size(&self) -> usize {
        0
    }
}
impl ValueSize for String {
    #[inline]
    fn size(&self) -> usize {
        self.capacity()
    }
}
