extern crate futures_mutex;
extern crate futures;

use futures_mutex::Mutex;
use futures::{future, Async, Future};

fn main() {
    let data = Mutex::new(2);
    data.clone().lock().map(|mut guard| *guard += 2).wait().unwrap();
    data.clone().lock().map(|guard| assert_eq!(*guard, 4)).wait().unwrap();
}
