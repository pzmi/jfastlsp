//! A test that fails on its first attempt and passes on a retry: the gate must report it red
//! (ver §2 rule 4: a rerun pass is a race to find, never a flake).

#[cfg(test)]
mod tests {
    #[test]
    fn should_fail_when_it_is_the_first_attempt() {
        // given
        let attempt = std::env::var("NEXTEST_ATTEMPT");

        // when
        let first = attempt.as_deref() == Ok("1");

        // then
        assert!(!first, "first attempt");
    }
}
