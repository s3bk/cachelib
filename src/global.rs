use tracing::info;
use tokio::sync::mpsc;
use std::sync::Weak;
use once_cell::sync::OnceCell;
use super::CacheControl;

pub struct GlobalCache {
    register: mpsc::UnboundedSender<Weak<dyn CacheControl>>,
}
static GLOBAL: OnceCell<GlobalCache> = OnceCell::new();

pub fn global_init() {
    use tokio::time::{Duration, sleep};

    let (tx, mut rx) = mpsc::unbounded_channel();
    GlobalCache { register: tx }.make_global();

    tokio::spawn(async move {
        let mut slots = vec![];
        let mut last_threshold = 0.0;
        let memory_limit = 1024 * 1024 * 1024;
        loop {
            while let Ok(new) = rx.try_recv() {
                slots.push(new);
            }

            let mut total_size = 0;
            let mut total_time = 0.0;
            slots.retain(|slot| {
                match Weak::upgrade(slot) {
                    None => false,
                    Some(cache) => {
                        let (size, time_sum) = cache.clean(last_threshold);
                        total_size += size;
                        total_time += time_sum;
                        true
                    }
                }
            });

            if total_size > 0 {
                let value = total_time / total_size as f32;
                let scale = total_size as f32 / memory_limit as f32;
                last_threshold = value * scale;
            } else {
                last_threshold = 0.0;
            }

            info!("{} seconds cached in {} bytes", total_time, total_size);
            info!("new threshold: {}", last_threshold);

            sleep(Duration::from_secs(1)).await;
        }
    });
}

impl GlobalCache {
    pub fn global() -> &'static Self {
        GLOBAL.get().unwrap()
    }
    pub fn make_global(self) {
        GLOBAL.set(self).ok().expect("");
    }
    pub fn register(cache: Weak<impl CacheControl>) {
        GLOBAL.get().unwrap().register.send(cache as _).ok().unwrap();
    }
}
