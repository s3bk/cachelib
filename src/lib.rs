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
use std::hash::Hash;
use std::collections::HashMap;
use std::sync::Mutex;

pub trait Cache<K, V> {
    fn new() -> Self;
    fn get(&self, key: K, compute: impl FnOnce() -> V) -> V;
}

#[cfg(feature="sync")]
pub mod sync;

#[cfg(feature="async")]
pub mod r#async;

#[cfg(feature="global")]
pub mod global;

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

pub trait CacheControl: Sync + Send + 'static {
    fn name(&self) -> Option<&str>;
    fn clean(&self, threshold: f32) -> (usize, f32);
}

impl<K, V> Cache<K, V> for Mutex<HashMap<K, V>>
    where K: Eq + Hash, V: Clone
{
    fn new() -> Self {
        Mutex::new(HashMap::new())
    }
    fn get(&self, key: K, compute: impl FnOnce() -> V) -> V {
        self.lock().unwrap().entry(key).or_insert_with(compute).clone()
    }
}
