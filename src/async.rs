use std::collections::hash_map::{HashMap, Entry};
use std::hash::Hash;
use std::mem::replace;
use std::sync::Arc;
use tokio::sync::{Notify, Mutex};
use tokio::time::Instant;
use super::{ValueSize, CacheControl, global::GlobalCache};
use std::future::{Future, ready};
use async_trait::async_trait;

struct Computed<V> {
    value: V,
    size: usize,
    time: f32,
    last_used: u32,
}
enum Value<V> {
    InProcess(Arc<Notify>),
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
pub struct AsyncCache<K, V> {
    name: Option<String>,
    inner: Mutex<CacheInner<K, V>>,
}
struct CacheInner<K, V> {
    entries: HashMap<K, Value<V>>,
    time_counter: u32,
    last_clean_timestamp: u32,
    size: usize,
}

impl<K, V> AsyncCache<K, V> 
    where K: Clone + Hash + Eq + Send + 'static, V: Clone + ValueSize + Send + 'static,
{
    pub fn new() -> Arc<Self> {
        Self::with_name(None)
    }
    pub fn with_name(name: Option<String>) -> Arc<Self> {
        let cache = Arc::new(AsyncCache {
            name: name.into(),
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
    pub fn entries(self) -> impl Iterator<Item=(K, V)> {
        self.inner.into_inner()
            .entries
            .into_iter()
            .map(|(k, v)| (k, v.unwrap()))
    }
    pub async fn get(&self, key: K, compute: impl FnOnce() -> V) -> V {
        self.get_async(key, || ready(compute())).await
    }
    pub async fn get_async<F, C>(&self, key: K, compute: C) -> V
    where
        F: Future<Output=V>,
        C: FnOnce() -> F
    {
        let mut guard = self.inner.lock().await;
        match guard.entries.entry(key) {
            Entry::Occupied(e) => match e.get() {
                &Value::Computed(ref v) => return v.value.clone(),
                &Value::InProcess(ref condvar) => {
                    let key = e.key().clone();
                    let condvar = condvar.clone();
                    drop(guard);
                    return self.poll(key, condvar).await;
                }
            }
            Entry::Vacant(e) => {
                let key = e.key().clone();
                let notify = Arc::new(Notify::new());
                e.insert(Value::InProcess(notify));
                drop(guard);

                let start = Instant::now();
                let value = compute().await;
                let size = value.size();
                let duration = start.elapsed();
                let value2 = value.clone();
                let time = duration.as_secs_f32() + 0.0000001;
                let mut guard = self.inner.lock().await;

                let c = Computed {
                    value,
                    size,
                    time,
                    last_used: guard.time_counter
                };
                guard.size += size;
                let slot = guard.entries.get_mut(&key).unwrap();
                let slot = replace(slot, Value::Computed(c));
                match slot {
                    Value::InProcess(ref notify) => notify.notify_waiters(),
                    _ => unreachable!()
                }
                return value2;
            }
        }
    }
    async fn poll(&self, key: K, notify: Arc<Notify>) -> V {
        loop {
            notify.notified().await;
            let mut guard = self.inner.lock().await;
            let inner = &mut *guard;
            if let &mut Value::Computed(ref mut v) = inner.entries.get_mut(&key).unwrap() {
                v.last_used = inner.time_counter;
                return v.value.clone();
            }
        }
    }
}

#[async_trait]
impl<K, V> CacheControl for AsyncCache<K, V>
    where K: Eq + Hash + Send + 'static, V: Clone + ValueSize + Send + 'static
{
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    async fn clean(&self, threshold: f32) -> (usize, f32) {
        self.inner.lock().await.clean(threshold)
    }
}

impl<K, V> CacheInner<K, V> {
    fn clean(&mut self, threshold: f32) -> (usize, f32) {
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
        self.size.checked_sub(freed).unwrap();
        self.last_clean_timestamp = self.time_counter;
        (self.size, time_sum)
    }
}
