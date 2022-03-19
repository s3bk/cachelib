use std::sync::{Mutex, Condvar, Arc, MutexGuard};
use std::collections::hash_map::{HashMap, Entry};
use std::hash::Hash;
use std::mem::replace;
use std::fmt::Debug;

enum Value<V> {
    InProcess(Arc<Condvar>),
    Computed(V),
}
impl<V> Value<V> {
    fn unwrap(self) -> V {
        match self {
            Value::Computed(v) => v,
            _ => unreachable!()
        }
    }
}
#[derive(Default)]
pub struct SyncCache<K, V> {
    entries: Mutex<HashMap<K, Value<V>>>
}
impl<K, V> SyncCache<K, V> 
    where K: Clone + Hash + Eq, V: Clone,
{
    pub fn new() -> Self {
        SyncCache {
            entries: Mutex::new(HashMap::new())
        }
    }
    pub fn entries(self) -> impl Iterator<Item=(K, V)> {
        self.entries.into_inner()
            .unwrap()
            .into_iter()
            .map(|(k, v)| (k, v.unwrap()))
    }
    pub fn get(&self, key: K, compute: impl FnOnce() -> V) -> V {
        let mut guard = self.entries.lock().unwrap();
        match guard.entry(key) {
            Entry::Occupied(e) => match e.get() {
                &Value::Computed(ref v) => return v.clone(),
                &Value::InProcess(ref condvar) => {
                    let key = e.key().clone();
                    let condvar = condvar.clone();
                    return self.poll(key, guard, condvar);
                }
            }
            Entry::Vacant(e) => {
                let key = e.key().clone();
                let condvar = Arc::new(Condvar::new());
                e.insert(Value::InProcess(condvar));
                drop(guard);

                let value = compute();

                let mut guard = self.entries.lock().unwrap();
                let slot = guard.get_mut(&key).unwrap();
                let slot = replace(slot, Value::Computed(value.clone()));
                match slot {
                    Value::InProcess(ref condvar) => condvar.notify_all(),
                    _ => unreachable!()
                }
                return value;
            }
        }
    }
    fn poll(&self, key: K, mut guard: MutexGuard<HashMap<K, Value<V>>>, condvar: Arc<Condvar>) -> V {
        loop {
            guard = condvar.wait(guard).unwrap();
            if let &Value::Computed(ref v) = guard.get(&key).unwrap() {
                return v.clone();
            }
        }
    }
}