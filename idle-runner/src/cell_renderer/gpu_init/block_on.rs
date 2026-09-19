pub(crate) fn block_on_future<F: std::future::Future>(future: F) -> F::Output {
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| spin_block_on(future))
    } else {
        spin_block_on(future)
    }
}

/// `futures_lite::future::block_on` equivalent: drive a future to completion
/// with a waker that unparks the calling thread (what `pollster`/`futures_lite`
/// do internally — wgpu futures wake through this when GPU work completes).
pub(crate) fn spin_block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn waker(thread: std::thread::Thread) -> Waker {
        let data = Arc::into_raw(Arc::new(thread)) as *const ();
        unsafe fn clone(d: *const ()) -> RawWaker {
            let arc = std::mem::ManuallyDrop::new(unsafe {
                Arc::from_raw(d as *const std::thread::Thread)
            });
            RawWaker::new(Arc::into_raw(Arc::clone(&arc)) as *const (), &VTABLE)
        }
        unsafe fn wake(d: *const ()) {
            unsafe { Arc::from_raw(d as *const std::thread::Thread) }.unpark();
        }
        unsafe fn wake_by_ref(d: *const ()) {
            std::mem::ManuallyDrop::new(unsafe { Arc::from_raw(d as *const std::thread::Thread) })
                .unpark();
        }
        unsafe fn drop(d: *const ()) {
            std::mem::drop(unsafe { Arc::from_raw(d as *const std::thread::Thread) });
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        unsafe { Waker::from_raw(RawWaker::new(data, &VTABLE)) }
    }

    let waker = waker(std::thread::current());
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => std::thread::park(),
        }
    }
}
