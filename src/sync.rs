use std::sync::{Mutex, Condvar, Arc, MutexGuard};
use std::collections::hash_map::{HashMap, Entry};
use std::hash::Hash;
use std::mem::replace;
use std::time::Instant;
use super::{ValueSize, CacheControl, global::GlobalCache};
use async_trait::async_trait;

struct Computed<V> {
    value: V,
    size: usize,
    time: f32,
    last_used: u32,
}
enum Value<V> {
    InProcess(Arc<Condvar>),
    Computed(Computed<V>),
}
impl<V> Value<V> {
    fn unwrap(self) -> V {
        match self {
            Value::Computed(v) => v.value,
            _ => unreachable!()
        }
    }
}
pub struct SyncCache<K, V> {
    name: Option<String>,
    inner: Mutex<CacheInner<K, V>>,
}
struct CacheInner<K, V> {
    entries: HashMap<K, Value<V>>,
    time_counter: u32,
    last_clean_timestamp: u32,
    size: usize,
}

impl<K, V> SyncCache<K, V> 
    where K: Hash + Eq + Clone + Send + 'static, V: Clone + ValueSize + Send + 'static,
{
    pub fn new() -> Arc<Self> {
        Self::with_name(None)
    }
    pub fn with_name(name: Option<String>) -> Arc<Self> {
        let cache = Arc::new(SyncCache {
            name,
            inner: Mutex::new(CacheInner {
                entries: HashMap::new(),
                time_counter: 1,
                last_clean_timestamp: 0,
                size: 0,
            })
        });
        GlobalCache::register(Arc::downgrade(&cache));
        cache
    }
    pub fn get(&self, key: K, compute: impl FnOnce() -> V) -> V {
        let mut guard = self.inner.lock().unwrap();
        match guard.entries.entry(key) {
            Entry::Occupied(e) => match e.get() {
                &Value::Computed(ref v) => return v.value.clone(),
                &Value::InProcess(ref condvar) => {
                    let key = e.key().clone();
                    let condvar = condvar.clone();
                    return Self::poll(key, guard, condvar);
                }
            }
            Entry::Vacant(e) => {
                let key = e.key().clone();
                let condvar = Arc::new(Condvar::new());
                e.insert(Value::InProcess(condvar));
                drop(guard);

                let start = Instant::now();
                let value = compute();
                let size = value.size();
                let duration = start.elapsed();
                let value2 = value.clone();
                let time = duration.as_secs_f32() + 0.0000001;
                let mut guard = self.inner.lock().unwrap();

                guard.size += size;
                let c = Computed {
                    value,
                    size,
                    time,
                    last_used: guard.time_counter
                };
                let slot = guard.entries.get_mut(&key).unwrap();
                let slot = replace(slot, Value::Computed(c));
                match slot {
                    Value::InProcess(ref condvar) => condvar.notify_all(),
                    _ => unreachable!()
                }
                return value2;
            }
        }
    }
    pub fn entries(arc: Arc<Self>) -> impl Iterator<Item=(K, V)> {
        let cache = Arc::try_unwrap(arc).ok().unwrap();
            cache.inner.into_inner().unwrap()
            .entries
            .into_iter()
            .map(|(k, v)| (k, v.unwrap()))
    }
    fn poll(key: K, mut guard: MutexGuard<CacheInner<K, V>>, condvar: Arc<Condvar>) -> V {
        loop {
            guard = condvar.wait(guard).unwrap();
            let inner = &mut *guard;
            if let &mut Value::Computed(ref mut v) = inner.entries.get_mut(&key).unwrap() {
                v.last_used = inner.time_counter;
                return v.value.clone();
            }
        }
    }
}

#[async_trait]
impl<K, V> CacheControl for SyncCache<K, V>
    where K: Eq + Hash + Send + 'static, V: Clone + ValueSize + Send + 'static
{
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    async fn clean(&self, threshold: f32) -> (usize, f32) {
        self.inner.lock().unwrap().clean(threshold)
    }
}

impl<K, V> CacheInner<K, V> {
    fn clean(&mut self, threshold: f32) -> (usize, f32) {
        self.time_counter += 1;
        let t2 = threshold / self.time_counter.wrapping_sub(self.last_clean_timestamp) as f32;
        let mut freed = 0;

        let mut time_sum = 0.0;
        self.entries.retain(|_, value| {
            match value {
                Value::Computed(ref entry) => {
                    let elapsed = self.time_counter.wrapping_sub(entry.last_used);
                    let value = entry.time / (entry.size as f32 * elapsed as f32);
                    if value > t2 {
                        time_sum += entry.time;
                        true
                    } else {
                        freed += entry.size;
                        false
                    }
                }
                Value::InProcess(_) => true
            }
        });
        self.size -= freed;
        self.last_clean_timestamp = self.time_counter;
        (self.size, time_sum)
    }
}
