# futures-mutex
A mutual exclusion future-powered synchronisation primitive useful for protecting shared data.

A mutex is designed to use along with `futures` crate. API is similar to
[`BiLock<T>`](https://docs.rs/futures/0.1/futures/sync/struct.BiLock.html)
from futures crate. Though, it can be cloned as long as you need.

## Installation

This crate can be included in your Cargo enabled project like this:

```toml
[dependencies]
futures-mutex = { git = "https://github.com/dekerakt/futures-mutex" }
```

Then include `futures-mutex` in your project like this:
```rust
extern crate futures_mutex;
```

## Examples
```rust
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
```

Note that `poll_lock` method of the mutex must be called inside the
context of the task, otherwise it will panic.

You can also call `lock` method of the mutex, which returns a future which resolves to
the owned-guard.

```rust
let data = Mutex::new(2);
data.clone().lock().map(|mut guard| *guard += 2).wait().unwrap();
data.clone().lock().map(|guard| assert_eq!(*guard, 4)).wait().unwrap();
```

You can see full source code of examples in `examples/` directory.

