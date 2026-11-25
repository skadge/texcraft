//! Running a forge operation: at once on the caller's thread ([`block_on`], the desktop's command
//! line and tests), or in the background ([`spawn`]: its own thread on the desktop, the page's
//! event loop in the browser) with [`Pending::poll`] giving its outcome once it has ended.

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

use crate::Result;

/// Runs `future` to its end on this thread, sleeping while it waits.
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<F: Future>(future: F) -> F::Output {
    struct Unpark(std::thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Unpark(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(v) = future.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

/// Runs `futures` together; their outputs in their order.
pub async fn join_all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut futures: Vec<Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
    let mut outputs: Vec<Option<F::Output>> = futures.iter().map(|_| None).collect();
    std::future::poll_fn(move |cx| {
        let mut waiting = false;
        for (future, output) in futures.iter_mut().zip(outputs.iter_mut()) {
            if output.is_none() {
                match future.as_mut().poll(cx) {
                    Poll::Ready(v) => *output = Some(v),
                    Poll::Pending => waiting = true,
                }
            }
        }
        if waiting { Poll::Pending } else { Poll::Ready(outputs.iter_mut().filter_map(Option::take).collect()) }
    })
    .await
}

/// An operation running in the background.
pub struct Pending<T> {
    #[cfg(not(target_arch = "wasm32"))]
    outcome: std::sync::mpsc::Receiver<Result<T>>,
    #[cfg(target_arch = "wasm32")]
    outcome: std::rc::Rc<std::cell::RefCell<Option<Result<T>>>>,
}

impl<T> Pending<T> {
    /// The outcome, once the operation has ended; none while it runs.
    pub fn poll(&mut self) -> Option<Result<T>> {
        #[cfg(not(target_arch = "wasm32"))]
        match self.outcome.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(crate::Error::Stopped)),
        }
        #[cfg(target_arch = "wasm32")]
        self.outcome.borrow_mut().take()
    }

    /// Waits for the outcome (the desktop only: a browser cannot wait).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(self) -> Result<T> {
        self.outcome.recv().unwrap_or(Err(crate::Error::Stopped))
    }
}

/// Starts the operation `make` returns, in the background: on its own thread (desktop), or on the
/// page's event loop (browser). `make` runs there, so what it builds (the transport) need not
/// cross threads.
pub fn spawn<F, Fut, T>(make: F) -> std::result::Result<Pending<T>, String>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T>> + 'static,
    T: Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("texcraft-forge".to_owned())
            .spawn(move || {
                let _ = tx.send(block_on(make()));
            })
            .map_err(|e| format!("The GitLab operation could not start: {e}"))?;
        Ok(Pending { outcome: rx })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
        let filled = slot.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let outcome = make().await;
            *filled.borrow_mut() = Some(outcome);
        });
        Ok(Pending { outcome: slot })
    }
}
