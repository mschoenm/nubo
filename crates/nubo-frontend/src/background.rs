//! Runs slow work, such as scanning folders, reading pictures, emulating a
//! game for its title screen or downloading box art, on a thread of its own.
//!
//! UI toolkits often run tasks on a small shared thread pool (Iced's also
//! drives the app's subscriptions), so blocking work there could slow down
//! the window. Work started here never waits on such a pool, and a slow
//! server can't hold up anything else. In the browser, which has a single
//! thread, the work runs when the future is first polled.

/// Starts `work` on a new thread and returns a future that resolves to its
/// result, for the app to wait on (with Iced, `Task::perform`), or to why
/// there is none: no thread could start, or the work panicked (Rust prints
/// the panic on stderr). Either way the message for the work still arrives,
/// so whatever waits for it, such as a job slot in the library, is freed.
#[cfg(not(target_arch = "wasm32"))]
pub fn run<T, F>(work: F) -> impl Future<Output = Result<T, String>> + use<T, F>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (sender, receiver) = futures::channel::oneshot::channel();
    let start_error = std::thread::Builder::new()
        .name(String::from("background"))
        .spawn(move || {
            let _ = sender.send(work());
        })
        .err();

    async move {
        if let Some(error) = start_error {
            return Err(format!("no thread could start for it ({error})"));
        }
        receiver
            .await
            .map_err(|_| String::from("it stopped unexpectedly"))
    }
}

/// The browser has a single thread, so the work runs when the future is
/// first polled.
#[cfg(target_arch = "wasm32")]
pub async fn run<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    Ok(work())
}

/// Lets the browser draw the page and handle input before slow work that
/// runs in pieces goes on, by waiting for a timer of zero milliseconds. On
/// the desktop, where slow work has a thread of its own, it returns at once.
pub async fn pause() {
    pause_for(0).await;
}

/// Waits `milliseconds` in the browser, letting it do everything else
/// meanwhile, such as before slow work that is held back looks again
/// whether it may go on. On the desktop it returns at once.
#[cfg_attr(
    not(all(target_arch = "wasm32", feature = "web")),
    expect(clippy::unused_async, reason = "the browser's version waits")
)]
pub async fn pause_for(milliseconds: i32) {
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    {
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            if let Some(window) = web_sys::window() {
                let _ = window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, milliseconds);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "web")))]
    let _ = milliseconds;
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::run;
    use futures::executor::block_on;

    #[test]
    fn work_runs_on_another_thread_and_returns_its_result() {
        let caller = std::thread::current().id();

        let (result, worker) = block_on(run(move || (6 * 7, std::thread::current().id()))).unwrap();

        assert_eq!(result, 42);
        assert_ne!(worker, caller);
    }

    #[test]
    fn work_that_panics_resolves_to_an_error() {
        let result = block_on(run(|| -> u8 { panic!("this work fails on purpose") }));

        assert!(result.is_err());
    }
}
