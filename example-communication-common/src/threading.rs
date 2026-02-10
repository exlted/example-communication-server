use std::sync::Arc;
use tokio::sync::Mutex;

pub type ThreadSafe<T> = Arc<Mutex<T>>;

pub fn make_thread_safe<T>(data: T) -> ThreadSafe<T> {
    Arc::new(Mutex::new(data))
}