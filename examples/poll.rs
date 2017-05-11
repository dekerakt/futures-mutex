extern crate futures_mutex;
extern crate futures;

use futures_mutex::Mutex;
use futures::{future, Async, Future};

fn main() {
    let future = future::lazy(|| {
        let lock1 = Mutex::new(0);

        // Mutex can be easily cloned as long as you need
        let lock2 = lock1.clone();

        assert!(lock1.poll_lock().is_ready());
        assert!(lock2.poll_lock().is_ready());

        let mut guard = match lock1.poll_lock() {
            Async::Ready(v) => v,
            Async::NotReady => unreachable!()
        };

        *guard += 1;

        assert!(lock1.poll_lock().is_not_ready());
        assert!(lock2.poll_lock().is_not_ready());

        drop(guard);

        assert!(lock1.poll_lock().is_ready());
        assert!(lock2.poll_lock().is_ready());
        Ok::<(), ()>(())
    });

    future.wait().unwrap();
}
