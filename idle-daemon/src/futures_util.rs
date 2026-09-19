// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! `futures_lite` replacements: a `Stream::next` awaitable built on
//! `std::future::poll_fn`. The `Stream` trait itself is re-exported from
//! zbus (which depends on `futures-core` anyway), so no futures crate is
//! needed in our dependency list.

use std::pin::Pin;

pub use zbus::export::futures_core::Stream;

/// `futures_lite::StreamExt::next` equivalent: yield the stream's next item.
pub async fn next<S: Stream + Unpin>(stream: &mut S) -> Option<S::Item> {
    std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::{Context, Poll};

    /// Vec-backed stream: yields items in order, then None forever.
    struct VecStream<T> {
        items: std::collections::VecDeque<T>,
    }
    impl<T> VecStream<T> {
        fn new(v: Vec<T>) -> Self {
            Self { items: v.into() }
        }
    }
    impl<T: Unpin> Stream for VecStream<T> {
        type Item = T;
        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<T>> {
            Poll::Ready(self.items.pop_front())
        }
    }

    /// Stream that parks once (Pending + wake), then yields — exercises the
    /// waker path through `poll_fn`, not just the immediate-Ready path.
    struct DeferredOnce {
        fired: bool,
    }
    impl Stream for DeferredOnce {
        type Item = u32;
        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<u32>> {
            if self.fired {
                return Poll::Ready(Some(9));
            }
            self.fired = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    #[test]
    fn next_yields_items_then_none() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut s = VecStream::new(vec![1, 2, 3]);
            assert_eq!(next(&mut s).await, Some(1));
            assert_eq!(next(&mut s).await, Some(2));
            assert_eq!(next(&mut s).await, Some(3));
            assert_eq!(next(&mut s).await, None);
            assert_eq!(next(&mut s).await, None, "drained stream stays None");
        });
    }

    #[test]
    fn next_survives_pending_then_ready() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut s = DeferredOnce { fired: false };
            assert_eq!(next(&mut s).await, Some(9));
        });
    }

    #[test]
    fn next_on_empty_stream_is_none() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut s: VecStream<u8> = VecStream::new(vec![]);
            assert_eq!(next(&mut s).await, None);
        });
    }
}
