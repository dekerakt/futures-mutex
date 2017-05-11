//! A mutual exclusion future-powered synchronization primitive useful for protecting shared data.
//!
//! # Examples
//!
//! ```
//! # extern crate futures_mutex;
//! # extern crate futures;
//! # use futures_mutex::Mutex;
//! # use futures::{future, Async, Future};
//! # fn main() {
//! # let future = future::lazy(|| {
//! let lock1 = Mutex::new(0);
//!
//! // Mutex can be easily cloned as long as you need
//! let lock2 = lock1.clone();
//!
//! assert!(lock1.poll_lock().is_ready());
//! assert!(lock2.poll_lock().is_ready());
//!
//! let mut guard = match lock1.poll_lock() {
//!     Async::Ready(v) => v,
//!     Async::NotReady => unreachable!()
//! };
//!
//! *guard += 1;
//!
//! assert!(lock1.poll_lock().is_not_ready());
//! assert!(lock2.poll_lock().is_not_ready());
//!
//! drop(guard);
//!
//! assert!(lock1.poll_lock().is_ready());
//! assert!(lock2.poll_lock().is_ready());
//! # Ok::<(), ()>(())
//! # });
//! # future.wait().unwrap();
//! # }
//! ```
//!
//! Note that `poll_lock` method of the mutex must be called inside the
//! context of the task, otherwise it will panic.
//!
//! You can also call `lock` method of the mutex, which returns a future which resolves to
//! the owned-guard.
//!
//! ```
//! # extern crate futures_mutex;
//! # extern crate futures;
//! # use futures_mutex::Mutex;
//! # use futures::{future, Async, Future};
//! # fn main() {
//! let data = Mutex::new(2);
//! data.clone().lock().map(|mut guard| *guard += 2).wait().unwrap();
//! data.clone().lock().map(|guard| assert_eq!(*guard, 4)).wait().unwrap();
//! # }
//! ```

extern crate futures;
extern crate crossbeam;

use std::mem;
use std::ops::{Deref, DerefMut};
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{Ordering, AtomicBool};

use crossbeam::sync::MsQueue;

use futures::{Future, Async, Poll};
use futures::task::{park, Task};

/// A mutex designed to use along with futures crate. API is similar to
/// [`futures::sync::BiLock`](https://docs.rs/futures/0.1/futures/sync/struct.BiLock.html).
/// Though, it can be cloned as long as you need.
#[derive(Debug, Clone)]
pub struct Mutex<T> {
    inner: Arc<Inner<T>>
}

#[derive(Debug)]
struct Inner<T> {
    inner: UnsafeCell<T>,
    locked: AtomicBool,
    queue: MsQueue<Task>
}

unsafe impl<T: Send> Send for Inner<T> {}
unsafe impl<T: Sync> Sync for Inner<T> {}

impl<T> Mutex<T> {
    /// Creates a new mutex in an unlocked state.
    pub fn new(t: T) -> Mutex<T> {
        let inner = Arc::new(Inner {
            inner: UnsafeCell::new(t),
            locked: AtomicBool::new(false),
            queue: MsQueue::new()
        });

        Mutex { inner }
    }

    /// Attempts to acquire mutex, returning `Async::NotReady` if other task already locked it.
    ///
    /// This function acquires the lock in a nonblocking fashion, returning immediately if lock is
    /// already held. If lock is succesefully acquired then `Async::Ready` is returned with
    /// `MutexGuard<T>`. The lock is unlocked when this `MutexGuard<T>` is dropped.
    ///
    /// # Panics
    ///
    /// This function will panic if called outside the context of a future's task.
    pub fn poll_lock(&self) -> Async<MutexGuard<T>> {
        if self.inner.locked.compare_and_swap(false, true, Ordering::SeqCst) {
            self.inner.queue.push(park());
            Async::NotReady
        } else {
            Async::Ready(MutexGuard { inner: self })
        }
    }

    /// Consume this lock into a future that resolves to a guard that allows access to the data.
    ///
    /// This function consumes the `Mutex<T>` and returns a future `MutexAcquire<T>`. The returned
    /// future will resolve to `MutexAcquired<T>` which represents a locked lock similarly to
    /// `MutexGuard<T>`.
    ///
    /// The returned future will never resolve to an error.
    pub fn lock(self) -> MutexAcquire<T> {
        MutexAcquire {
            inner: self
        }
    }

    fn unlock(&self) {
        if !self.inner.locked.swap(false, Ordering::SeqCst) {
            panic!("trying to unlock unlocked mutex");
        }

        while !self.inner.locked.load(Ordering::SeqCst) {
            match self.inner.queue.try_pop() {
                Some(task) => task.unpark(),
                None => return
            }
        }
    }
}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        assert_eq!(self.locked.load(Ordering::SeqCst), false)
    }
}

/// Returned RAII guard from the `poll_lock` method.
///
/// This structure acts as a sentinel to the data in the `Mutex<T>` itself, implementing `Deref`
/// and `DerefMut` to `T`. When dropped, the lock will be unlocked.
#[derive(Debug)]
pub struct MutexGuard<'a, T: 'a> {
    inner: &'a Mutex<T>
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.inner.inner.inner.get() }
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.inner.inner.inner.get() }
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.inner.unlock()
    }
}

/// Future returned by `Mutex::lock` which will resolve when the lock is acquired.
#[derive(Debug)]
pub struct MutexAcquire<T> {
    inner: Mutex<T>
}

impl<T> Future for MutexAcquire<T> {
    type Item = MutexAcquired<T>;
    type Error = ();

    fn poll(&mut self) -> Poll<MutexAcquired<T>, ()> {
        match self.inner.poll_lock() {
            Async::Ready(r) => {
                mem::forget(r);
                Ok(MutexAcquired {
                    inner: Mutex { inner: self.inner.inner.clone() }
                }.into())
            }

            Async::NotReady => Ok(Async::NotReady)
        }
    }
}

/// Resolved value of the `MutexAcquire<T>` future.
///
/// This value, like `MutexGuard<T>`, is a sentinel to the value `T` through implementations of
/// `Deref` and `DerefMut` traits. When dropped will unlock the lock.
#[derive(Debug)]
pub struct MutexAcquired<T> {
    inner: Mutex<T>
}

impl<T> Deref for MutexAcquired<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.inner.inner.inner.get() }
    }
}

impl<T> DerefMut for MutexAcquired<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.inner.inner.inner.get() }
    }
}

impl<T> Drop for MutexAcquired<T> {
    fn drop(&mut self) {
        self.inner.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future;

    #[test]
    fn simple() {
        let future = future::lazy(|| {
            let lock1 = Mutex::new(vec![]);
            let lock2 = lock1.clone();
            let lock3 = lock1.clone();

            let mut guard = match lock1.poll_lock() {
                Async::Ready(v) => v,
                Async::NotReady => panic!("mutex locked")
            };

            assert_eq!(guard.len(), 0);
            guard.push(32);
            assert_eq!(guard.len(), 1);

            assert!(lock1.poll_lock().is_not_ready());
            assert!(lock2.poll_lock().is_not_ready());
            assert!(lock3.poll_lock().is_not_ready());

            drop(guard);

            assert!(lock1.poll_lock().is_ready());
            assert!(lock2.poll_lock().is_ready());
            assert!(lock3.poll_lock().is_ready());

            let mut guard = match lock2.poll_lock() {
                Async::Ready(v) => v,
                Async::NotReady => panic!("mutex locked")
            };

            assert_eq!(guard.len(), 1);
            guard.remove(0);
            assert_eq!(guard.len(), 0);

            assert!(lock1.poll_lock().is_not_ready());
            assert!(lock2.poll_lock().is_not_ready());
            assert!(lock3.poll_lock().is_not_ready());

            Ok::<(), ()>(())
        });

        future.wait().unwrap();
    }
}
