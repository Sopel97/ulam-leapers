use crate::util::cancel::{Canceled, CancellationToken};
use std::sync::{OnceLock, RwLock};

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum AsyncValueError {
    ValueAlreadyPresent,
    ConstructionUnderway,
}

struct ExecutorState<T> {
    worker: Option<std::thread::JoinHandle<Result<T, Canceled>>>,
    cancellation_token: CancellationToken,
}

impl<T> Drop for ExecutorState<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.worker.take() {
            // Cancellation is idempotent, no need to check if it's needed.
            self.cancellation_token.cancel();
            // Either ok or cancellation result. Don't care.
            let _res = handle.join().expect("Error joining worker thread");
        }
    }
}

pub struct AsyncValue<T> {
    // We want the structure to be as small as possible.
    // The executor state is only ephemerally required,
    // so don't bloat the size with it.
    executor_state: RwLock<Option<Box<ExecutorState<T>>>>,
    value: OnceLock<T>,
}

impl<T> Default for AsyncValue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> AsyncValue<T> {
    pub fn new() -> Self {
        Self {
            executor_state: RwLock::new(None),
            value: OnceLock::new(),
        }
    }

    fn poll(&self) {
        if self.value.get().is_some() {
            return;
        }

        let mut worker_finished = false;

        // Careful not to overextend the read lock.
        {
            let executor_state = self.executor_state.read().unwrap();
            if let Some(executor_state) = executor_state.as_ref()
                && let Some(worker) = executor_state.worker.as_ref()
            {
                worker_finished = worker.is_finished();
            }
        }

        if worker_finished {
            // Reacquire the lock, now with write permission because we need to retrieve
            // the result and completely dismantle the executor, leaving it `None`.
            if let Some(mut executor_state) = self.executor_state.write().unwrap().take()
                && let Some(worker) = executor_state.worker.take()
                && let Ok(result) = worker.join().unwrap()
            {
                self.value
                    .set(result)
                    .unwrap_or_else(|_| panic!("Value already present!"))
            };
        }
    }

    pub fn get(&self) -> Option<&T> {
        self.poll();

        self.value.get()
    }

    pub fn is_ready(&self) -> bool {
        self.poll();

        self.value.get().is_some()
    }

    /// Returns `true` if it's empty and there is no running constructor.
    pub fn is_empty_and_idle(&self) -> bool {
        self.value.get().is_none() && self.executor_state.read().unwrap().is_none()
    }

    /// If there is a constructor running it attempts its cancellation and blocks until
    /// the executor is dismantled. Even if the constructor has finished and yielded a result,
    /// though that result has not yet been finalized, the cancellation takes precedence.
    /// If there is no constructor running the function does nothing.
    /// `try_set_with` can be successfully called immediately after this function returns.
    ///
    /// NOTE: No value is returned to specify which case happened because the user should
    ///       be using `is_ready` to determine the actual state.
    pub fn try_cancel(&mut self) {
        // We don't call `self.poll()` because we want cancellation to take precedence.

        // We completely dismantle the executor state, leaving it `None` if it isn't already.
        if let Some(mut executor_state) = self.executor_state.write().unwrap().take()
            && let Some(handle) = executor_state.worker.take()
        {
            // Cancellation is idempotent, no need to check if it's needed.
            executor_state.cancellation_token.cancel();
            // Should be Canceled but we don't really care.
            let _res = handle.join().unwrap();
        }
    }
}

impl<T> AsyncValue<T>
where
    T: Send + Sync + 'static,
{
    /// Schedules an asynchronous construction of the underlying value.
    /// The constructor takes a cancellation token, which it may ignore if
    /// cancellation is not required or if blocking for the whole duration of
    /// the computation on cancellation is acceptable.
    ///
    /// The function returns an `Err(error)` if a value is already present or under construction.
    pub fn try_set_with<F>(&mut self, constructor: F) -> Result<(), AsyncValueError>
    where
        F: FnOnce(CancellationToken) -> Result<T, Canceled> + Sync + Send + 'static,
    {
        if self.is_ready() {
            return Err(AsyncValueError::ValueAlreadyPresent);
        }

        if self.executor_state.read().unwrap().is_some() {
            return Err(AsyncValueError::ConstructionUnderway);
        }

        let cancellation_token = CancellationToken::new();
        let cancellation_token_for_worker = cancellation_token.clone();

        let executor = Some(Box::new(ExecutorState {
            cancellation_token,
            worker: Some(std::thread::spawn(move || {
                constructor(cancellation_token_for_worker)
            })),
        }));

        *self.executor_state.write().unwrap() = executor;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::cancel::Canceled;
    use std::sync::{Arc, Barrier};
    use std::thread::sleep;
    use std::time::Duration;

    /// Spin-waits up to `timeout` for `predicate` to return true.
    fn wait_until(timeout: Duration, predicate: impl Fn() -> bool) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if predicate() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    const TIMEOUT: Duration = Duration::from_secs(1);

    #[test]
    fn new_value_is_not_ready() {
        let av: AsyncValue<i32> = AsyncValue::new();
        assert!(av.get().is_none());
        assert!(!av.is_ready());
    }

    #[test]
    fn default_matches_new() {
        let av: AsyncValue<i32> = AsyncValue::default();
        assert!(av.get().is_none());
        assert!(!av.is_ready());
    }

    #[test]
    fn value_becomes_ready_after_worker_finishes() {
        let mut av: AsyncValue<u32> = AsyncValue::new();
        av.try_set_with(|_token| Ok(42)).unwrap();

        assert!(wait_until(TIMEOUT, || av.is_ready()), "timed out waiting for value");
        assert_eq!(*av.get().unwrap(), 42);
    }

    #[test]
    fn get_returns_same_value_after_resolution() {
        let mut av: AsyncValue<String> = AsyncValue::new();
        av.try_set_with(|_| Ok("hello".to_string())).unwrap();

        assert!(wait_until(TIMEOUT, || av.is_ready()));
        assert_eq!(av.get(), av.get());
        assert_eq!(av.get().unwrap(), "hello");
    }

    #[test]
    fn value_survives_multiple_get_calls() {
        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_set_with(|_| Ok(99)).unwrap();

        assert!(wait_until(TIMEOUT, || av.get().is_some()));
        for _ in 0..10 {
            assert_eq!(*av.get().unwrap(), 99);
        }
    }

    #[test]
    fn value_not_ready_while_worker_is_running() {
        let barrier = Arc::new(Barrier::new(2));
        let barrier_clone = Arc::clone(&barrier);

        let mut av: AsyncValue<u32> = AsyncValue::new();
        av.try_set_with(move |_token| {
            barrier_clone.wait(); // signal: worker has started
            barrier_clone.wait(); // wait for test to release us
            Ok(7)
        })
            .unwrap();

        barrier.wait(); // wait until worker has started
        assert!(!av.is_ready(), "value should not be ready while worker is blocked");

        barrier.wait(); // unblock worker
        assert!(wait_until(TIMEOUT, || av.is_ready()));
        assert_eq!(*av.get().unwrap(), 7);
    }

    #[test]
    fn try_set_with_errors_if_value_already_present() {
        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_set_with(|_| Ok(1)).unwrap();

        assert!(wait_until(TIMEOUT, || av.is_ready()));

        let result = av.try_set_with(|_| Ok(2));
        assert!(
            matches!(result, Err(AsyncValueError::ValueAlreadyPresent)),
            "expected ValueAlreadyPresent, got {:?}",
            result
        );

        // Original value is unchanged.
        assert_eq!(*av.get().unwrap(), 1);
    }

    #[test]
    fn try_set_with_errors_if_construction_underway() {
        let barrier = Arc::new(Barrier::new(2));
        let barrier_clone = Arc::clone(&barrier);

        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_set_with(move |_| {
            barrier_clone.wait(); // signal: started
            barrier_clone.wait(); // hold until released
            Ok(1)
        })
            .unwrap();

        barrier.wait(); // wait until first worker is running

        let result = av.try_set_with(|_| Ok(2));
        assert!(
            matches!(result, Err(AsyncValueError::ConstructionUnderway)),
            "expected ConstructionUnderway, got {:?}",
            result
        );

        barrier.wait(); // unblock first worker so drop doesn't deadlock
    }

    #[test]
    fn try_set_with_succeeds_on_fresh_instance() {
        let mut av: AsyncValue<i32> = AsyncValue::new();
        assert!(av.try_set_with(|_| Ok(5)).is_ok());
        assert!(wait_until(TIMEOUT, || av.is_ready()));
    }

    #[test]
    fn try_cancel_on_empty_does_nothing() {
        // Should not panic or block.
        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_cancel();
        assert!(!av.is_ready());
    }

    #[test]
    fn try_cancel_stops_running_worker() {
        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_set_with(|ct| {
            while !ct.is_canceled() {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(Canceled)
        })
            .unwrap();

        av.try_cancel(); // must return (worker joined)
        assert!(!av.is_ready(), "value should not be set after cancellation");
    }

    #[test]
    fn try_cancel_supersedes_finished_worker() {
        // Even if the worker has produced a value, try_cancel should discard it
        // (cancellation takes precedence over finalization per the doc comment).
        let barrier = Arc::new(Barrier::new(2));
        let barrier_clone = Arc::clone(&barrier);

        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_set_with(move |_| {
            barrier_clone.wait(); // signal: work done, but poll() not yet called
            Ok(42)
        })
            .unwrap();

        // We can't get the exact moment the worker finishes,
        // but we can get close with a barrier + wait.
        barrier.wait();
        sleep(Duration::from_millis(50));

        // Don't call is_ready() / get() - cancel before poll() can finalize the value.
        av.try_cancel();

        assert!(!av.is_ready(), "try_cancel should prevent the value from being stored");
    }

    #[test]
    fn try_set_with_succeeds_after_try_cancel() {
        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_set_with(|ct| {
            while !ct.is_canceled() {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(Canceled)
        })
            .unwrap();

        av.try_cancel();

        // Now try_set_with should be accepted again.
        assert!(
            av.try_set_with(|_| Ok(99)).is_ok(),
            "try_set_with should succeed after try_cancel"
        );
        assert!(wait_until(TIMEOUT, || av.is_ready()));
        assert_eq!(*av.get().unwrap(), 99);
    }

    #[test]
    fn drop_cancels_and_joins_running_worker() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        {
            let mut av: AsyncValue<i32> = AsyncValue::new();
            av.try_set_with(move |ct| {
                while !ct.is_canceled() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                drop(tx); // signals that the worker actually exited
                Err(Canceled)
            })
                .unwrap();
            // `av` is dropped here -> Drop must cancel + join the worker.
        }

        // If drop properly joined the thread, `tx` was dropped and `rx.recv()`
        // returns an error (channel closed). A timeout means drop didn't join.
        rx.recv_timeout(TIMEOUT)
            .expect_err("worker thread should have been joined by drop");
    }

    #[test]
    fn concurrent_get_calls_are_safe() {
        let mut av: AsyncValue<u64> = AsyncValue::new();
        av.try_set_with(|_| Ok(123)).unwrap();

        let av = Arc::new(av);
        let mut handles = Vec::new();

        for _ in 0..8 {
            let av = Arc::clone(&av);
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    let _ = av.get();
                    std::thread::yield_now();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert!(wait_until(TIMEOUT, || av.is_ready()));
        assert_eq!(*av.get().unwrap(), 123);
    }

    #[test]
    fn works_with_non_copy_type() {
        let mut av: AsyncValue<Vec<String>> = AsyncValue::new();
        av.try_set_with(|_| Ok(vec!["a".to_string(), "b".to_string()])).unwrap();

        assert!(wait_until(TIMEOUT, || av.is_ready()));
        assert_eq!(av.get().unwrap(), &vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn works_with_large_value() {
        let mut av: AsyncValue<Vec<u8>> = AsyncValue::new();
        av.try_set_with(|_| Ok(vec![0u8; 10_000_000])).unwrap();

        assert!(wait_until(TIMEOUT, || av.is_ready()));
        assert_eq!(av.get().unwrap().len(), 10_000_000);
    }

    #[test]
    fn new_is_empty_and_idle() {
        let av: AsyncValue<i32> = AsyncValue::new();
        assert!(av.is_empty_and_idle());
    }

    #[test]
    fn is_empty_and_idle_after_cancelation() {
        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_set_with(|_| Ok(123)).unwrap();
        av.try_cancel();
        assert!(av.is_empty_and_idle());
    }

    #[test]
    fn is_not_empty_and_idle_after_construction() {
        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_set_with(|_| Ok(123)).unwrap();
        av.get(); // for poll
        assert!(!av.is_empty_and_idle());
    }

    #[test]
    fn is_not_empty_and_idle_during_construction() {
        let barrier = Arc::new(Barrier::new(2));
        let barrier_clone = Arc::clone(&barrier);

        let mut av: AsyncValue<i32> = AsyncValue::new();
        av.try_set_with(move |_| {
            barrier_clone.wait(); // signal: started
            barrier_clone.wait(); // hold until released
            Ok(1)
        })
            .unwrap();

        barrier.wait(); // wait until first worker is running

        assert!(!av.is_empty_and_idle());

        barrier.wait(); // unblock first worker so drop doesn't deadlock
    }
}