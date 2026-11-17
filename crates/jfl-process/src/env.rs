//! Where inherited variables come from: injected, so a test (and the contract suite) can hand the
//! runner a parent environment of its choosing without touching the process's own.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

/// The parent environment a [`crate::Command::inherit_env`] name is read from.
pub trait EnvSource {
    /// The value of `name`, or `None` when it is not set.
    fn var_os(&self, name: &OsStr) -> Option<OsString>;
}

/// The process's own environment (`std::env::var_os`): the source production uses.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn var_os(&self, name: &OsStr) -> Option<OsString> {
        std::env::var_os(name)
    }
}

/// A fixed environment, for tests and for callers that compute the parent's view themselves.
impl EnvSource for BTreeMap<OsString, OsString> {
    fn var_os(&self, name: &OsStr) -> Option<OsString> {
        self.get(name).cloned()
    }
}
