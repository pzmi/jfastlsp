//! `/proc/<pid>/status` memory lines, where the platform has them.
//!
//! Peak RSS is the kernel's high-water mark (`VmHWM`), steady RSS the current resident set
//! (`VmRSS`). macOS has no `/proc`: every function here then answers `None` and the metrics that
//! need it are left out of the report with a note, rather than approximated.

use std::path::Path;

/// The two memory lines of a `status` file, in KiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemStatus {
    /// `VmHWM`: peak resident set so far.
    pub peak_kib: Option<u64>,
    /// `VmRSS`: resident set now.
    pub rss_kib: Option<u64>,
}

/// Parse the text of a `/proc/<pid>/status` file.
pub fn parse_status(text: &str) -> MemStatus {
    let mut out = MemStatus::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            out.peak_kib = kib(rest);
        } else if let Some(rest) = line.strip_prefix("VmRSS:") {
            out.rss_kib = kib(rest);
        }
    }
    out
}

fn kib(rest: &str) -> Option<u64> {
    rest.split_whitespace().next()?.parse().ok()
}

/// The memory status of process `pid`, or of this process for `None`.
pub fn read_status(pid: Option<u32>) -> Option<MemStatus> {
    let path = match pid {
        Some(pid) => format!("/proc/{pid}/status"),
        None => "/proc/self/status".to_string(),
    };
    let text = std::fs::read_to_string(Path::new(&path)).ok()?;
    Some(parse_status(&text))
}

/// Parse a `/proc/<pid>/task/<pid>/children` file: the pids of a thread's children, separated
/// by spaces. A word that is not a pid makes the whole answer `None`.
pub fn parse_children(text: &str) -> Option<Vec<u32>> {
    text.split_whitespace().map(|w| w.parse().ok()).collect()
}

/// The children of process `pid` (its main thread's), where `/proc` has them. The jdtls arm
/// finds the JVM this way: it is the one child of the `timeout` the harness starts.
pub fn children(pid: u32) -> Option<Vec<u32>> {
    let path = format!("/proc/{pid}/task/{pid}/children");
    parse_children(&std::fs::read_to_string(Path::new(&path)).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_peak_and_current_when_the_status_has_both_lines() {
        // given
        let text = "Name:\tjfl-bench\nVmPeak:\t  99999 kB\nVmHWM:\t   15360 kB\nVmRSS:\t   12288 kB\nThreads:\t5\n";
        // when
        let s = parse_status(text);
        // then
        assert_eq!(s.peak_kib, Some(15360));
        assert_eq!(s.rss_kib, Some(12288));
    }

    #[test]
    fn should_answer_none_when_the_lines_are_absent_or_malformed() {
        // given
        let text = "Name:\tkernel-thread\nVmHWM:\tmany kB\n";
        // when
        let s = parse_status(text);
        // then
        assert_eq!(s, MemStatus::default());
    }

    #[test]
    fn should_read_each_pid_when_parsing_a_children_file() {
        // given
        let text = "4242 4250 \n";
        // when
        let pids = parse_children(text);
        // then
        assert_eq!(pids, Some(vec![4242, 4250]));
        assert_eq!(parse_children(""), Some(vec![]));
        assert_eq!(parse_children("12 x"), None);
        assert_eq!(parse_children("-1"), None);
    }
}
