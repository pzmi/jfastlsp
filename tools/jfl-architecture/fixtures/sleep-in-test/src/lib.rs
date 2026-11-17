//! Negative case of `ARCH-test-sleep` (fnd §2.4, ver §3): a test that sleeps outside
//! `jfl_testkit::poll`, which clippy with the root `clippy.toml` refuses.

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
}
