//! A wake-up flag shared between a producer thread and an async consumer.
//!
//! Any number of [`Signal::notify`] calls made before the consumer runs
//! collapse into a single wake-up, so a burst of output costs one redraw.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

#[derive(Default)]
struct State {
    pending: bool,
    closed: bool,
    waker: Option<Waker>,
}

/// Cloneable handle; all clones share the same flag.
#[derive(Clone, Default)]
pub struct Signal(Arc<Mutex<State>>);

impl Signal {
    /// Marks the signal as pending and wakes the waiting consumer, if any.
    pub fn notify(&self) {
        self.update(|state| state.pending = true);
    }

    /// Marks the producer as finished. Pending notifications are still delivered.
    pub fn close(&self) {
        self.update(|state| state.closed = true);
    }

    /// Resolves to `true` on the next notification, or `false` once the
    /// signal is closed with nothing pending.
    pub fn wait(&self) -> Wait<'_> {
        Wait(self)
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        let waker = {
            let mut state = self.0.lock().expect("signal lock poisoned");
            change(&mut state);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// Future returned by [`Signal::wait`].
pub struct Wait<'a>(&'a Signal);

impl Future for Wait<'_> {
    type Output = bool;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
        let mut state = self.0.0.lock().expect("signal lock poisoned");
        if state.pending {
            state.pending = false;
            Poll::Ready(true)
        } else if state.closed {
            Poll::Ready(false)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;

    struct CountingWaker(AtomicUsize);

    impl Wake for CountingWaker {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn poll(signal: &Signal, waker: &Arc<CountingWaker>) -> Poll<bool> {
        let waker = Waker::from(waker.clone());
        let mut wait = signal.wait();
        Pin::new(&mut wait).poll(&mut Context::from_waker(&waker))
    }

    #[test]
    fn notifications_coalesce_into_one_wake() {
        let signal = Signal::default();
        let waker = Arc::new(CountingWaker(AtomicUsize::new(0)));
        assert_eq!(poll(&signal, &waker), Poll::Pending);

        signal.notify();
        signal.notify();
        assert_eq!(waker.0.load(Ordering::SeqCst), 1);
        assert_eq!(poll(&signal, &waker), Poll::Ready(true));
        assert_eq!(poll(&signal, &waker), Poll::Pending);
    }

    #[test]
    fn close_delivers_pending_notification_first() {
        let signal = Signal::default();
        let waker = Arc::new(CountingWaker(AtomicUsize::new(0)));
        signal.notify();
        signal.close();
        assert_eq!(poll(&signal, &waker), Poll::Ready(true));
        assert_eq!(poll(&signal, &waker), Poll::Ready(false));
    }
}
