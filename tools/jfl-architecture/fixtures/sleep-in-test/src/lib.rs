//! Negative case of `ARCH-test-sleep` (fnd §2.4, ver §3): a test that sleeps outside
//! `jfl_testkit::poll`, which clippy with the root `clippy.toml` refuses; and the covert sleeps
//! the FND-05 checkpoint found (N7), timed waits on a channel nobody sends to and on a condition
//! variable nobody notifies, which the sleep grant covers too (FND-08).

/// The value under test.
pub fn answer() -> u8 {
    42
}

#[cfg(test)]
mod tests {
    #[test]
    fn should_answer_when_asked_after_a_sleep() {
        // given
        std::thread::sleep(core::time::Duration::from_millis(1));

        // when
        let answer = super::answer();

        // then
        assert_eq!(answer, 42);
    }

    #[test]
    fn should_answer_when_asked_after_a_timed_wait() {
        // given
        let (_tx, rx) = std::sync::mpsc::sync_channel::<()>(0);
        let waited = rx.recv_timeout(core::time::Duration::from_millis(1));
        let pair = (std::sync::Mutex::new(()), std::sync::Condvar::new());
        let guard = pair.0.lock().unwrap();
        let _ = pair.1.wait_timeout(guard, core::time::Duration::from_millis(1));

        // when
        let answer = super::answer();

        // then
        assert!(waited.is_err());
        assert_eq!(answer, 42);
    }
}
