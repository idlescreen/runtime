use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

#[test]
fn spin_block_on_ready_future_returns_value() {
    assert_eq!(spin_block_on(async { 42 }), 42);
    assert_eq!(spin_block_on(std::future::ready("ok")), "ok");
}

#[test]
fn spin_block_on_pending_future_completes_on_wake() {
    // A future that is Pending once, then Ready after a spawned thread
    // wakes the parking thread — proves the RawWaker unparks us.
    struct Once {
        fired: Arc<AtomicBool>,
    }
    impl std::future::Future for Once {
        type Output = u32;
        fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
            if self.fired.swap(true, Ordering::SeqCst) {
                return Poll::Ready(7);
            }
            let waker = cx.waker().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                waker.wake();
            });
            Poll::Pending
        }
    }
    let fired = Arc::new(AtomicBool::new(false));
    let out = spin_block_on(Once {
        fired: Arc::clone(&fired),
    });
    assert_eq!(out, 7);
}

#[test]
fn waker_clone_and_wake_by_ref_are_sound() {
    // Poll a future that clones its waker and calls wake_by_ref:
    // clone must add a ref (not alias) and wake_by_ref must not
    // consume the original — exercised by repeated parking.
    struct MultiWake {
        polls: Arc<AtomicUsize>,
    }
    impl std::future::Future for MultiWake {
        type Output = ();
        fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            let n = self.polls.fetch_add(1, Ordering::SeqCst);
            if n >= 2 {
                return Poll::Ready(());
            }
            let w1 = cx.waker().clone();
            let w2 = w1.clone();
            std::thread::spawn(move || {
                w2.wake_by_ref();
                w1.wake_by_ref();
                w1.wake();
            });
            Poll::Pending
        }
    }
    let polls = Arc::new(AtomicUsize::new(0));
    spin_block_on(MultiWake {
        polls: Arc::clone(&polls),
    });
    assert!(polls.load(Ordering::SeqCst) >= 3);
}

#[test]
fn block_on_future_works_outside_tokio() {
    assert_eq!(block_on_future(async { "plain" }), "plain");
}

#[test]
fn block_on_future_works_inside_tokio() {
    // block_in_place requires a multi-thread runtime.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let out = rt.block_on(async { block_on_future(async { 9u32 }) });
    assert_eq!(out, 9);
}
