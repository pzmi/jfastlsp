//! The cache keys of plan §5.4, derived from digests the caller supplies and hashed by the
//! caller's [`KeyHasher`] (blake3 in `jfl-adapter-store`, ADR 0025).
//!
//! | Key | Derived from |
//! |---|---|
//! | [`SyntaxKey`] | the file's bytes digest, the grammar's crate name and exact version, the summariser version |
//! | [`EnvironmentKey`] | the ordered classpath and module path (each entry by content digest), the JDK (vendor, version, `release` file digest), `--release`, the semantic compiler flags in order, the annotation-processor path and options |
//! | [`SemanticKey`] | the file's `SyntaxKey`, the `EnvironmentKey`, and the set of declaration API digests its resolution read |
//!
//! **What a `SemanticKey` reads** (plan §5.4: "the digest of every declaration summary its
//! resolution read"). The digests are of declaration *APIs*
//! ([`FileSummary::api_digest`](crate::summary::FileSummary::api_digest)), never of whole files:
//! a method-body edit in a type that 1,586 files depend on (BENCH-03's fan-out) changes that
//! file's `SyntaxKey` and its own `SemanticKey`, and no dependent's. A type's API covers its
//! direct members' signatures and a member's API its owners' headers, so a new overload, an
//! added annotation or a changed bound invalidates whoever read them. Four conditions keep this
//! sound (plan §5.4 states them in full) and bind the resolver (SEM-01) that records the reads:
//!
//! 1. Every scope consulted to bind a name *and prove it unique* is a read, as the digest of its
//!    name set (a package's top-level type names, an on-demand import's member type names), and
//!    so is every candidate rejected for access; a miss is the digests of every scope it was
//!    looked up in, so a type added later to any of them changes the key.
//! 2. A member's read includes its owner type's read.
//! 3. A cached resolution result names declarations by [`SymbolId`](crate::SymbolId), never by
//!    byte range: ranges are not in the API digest, and a result is placed in its file at query
//!    time from the current summary.
//! 4. A `SemanticKey` keys name-binding results only, never `javac` diagnostics, constant values
//!    (field initializers are outside the API) or Javadoc.
//!
//! The reads are a set: their order and repetitions do not change the key
//! (`keys::tests::should_derive_one_semantic_key_when_reads_are_reordered_or_repeated`).

use alloc::vec::Vec;
use core::fmt;

use crate::digest::{Digest, Encoder, KeyHasher};

macro_rules! key {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Digest);

        impl $name {
            /// The key's digest: its name in the store.
            #[must_use]
            pub const fn digest(&self) -> &Digest {
                &self.0
            }

            /// The key whose digest this is (read back from the store).
            #[must_use]
            pub const fn from_digest(digest: Digest) -> Self {
                Self(digest)
            }
        }

        /// The digest in lower-case hexadecimal.
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

key!(
    /// Shares a file's syntax summary across every worktree and commit holding the same bytes.
    ///
    /// Pinned by: `keys::tests::should_change_each_key_when_any_of_its_inputs_changes`.
    SyntaxKey
);
key!(
    /// Shares `javac` workers, library indexes and semantic results across files compiled alike.
    ///
    /// Pinned by: `keys::tests::should_change_each_key_when_any_of_its_inputs_changes`.
    EnvironmentKey
);
key!(
    /// A file's resolution results, valid only within one [`EnvironmentKey`].
    ///
    /// Pinned by: `keys::tests::should_change_each_key_when_any_of_its_inputs_changes` and
    /// `keys::tests::should_derive_one_semantic_key_when_reads_are_reordered_or_repeated`.
    SemanticKey
);

/// The inputs of a [`SyntaxKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxInputs<'a> {
    /// The digest of the file's bytes (after a leading BOM is dropped, IDX-07).
    pub file: Digest,
    /// The grammar's crate name, such as `tree-sitter-java-orchard`.
    pub grammar: &'a str,
    /// The grammar crate's exact version.
    pub grammar_version: &'a str,
    /// The summariser's own format version: bumped whenever extraction changes its output.
    pub summariser_version: u32,
}

impl SyntaxKey {
    /// The key of `inputs`.
    pub fn derive<H: KeyHasher>(inputs: &SyntaxInputs<'_>, hasher: H) -> Self {
        let mut encoder = Encoder::new(hasher, "jfl.syntax-key.v1");
        encoder.digest(&inputs.file);
        encoder.str(inputs.grammar);
        encoder.str(inputs.grammar_version);
        encoder.u32(inputs.summariser_version);
        Self(encoder.finish())
    }
}

/// The JDK a module compiles against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JdkIdentity<'a> {
    /// The vendor, such as `Eclipse Adoptium`.
    pub vendor: &'a str,
    /// The full version, such as `21.0.5+11`.
    pub version: &'a str,
    /// The digest of the JDK's `release` file.
    pub release_file: Digest,
}

/// The inputs of an [`EnvironmentKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvironmentInputs<'a> {
    /// The classpath entries' content digests, in classpath order (order decides shadowing).
    pub classpath: &'a [Digest],
    /// The module path entries' content digests, in order.
    pub module_path: &'a [Digest],
    /// The JDK.
    pub jdk: JdkIdentity<'a>,
    /// `--release N`, if given.
    pub release: Option<u32>,
    /// The flags that change what compiles (`-parameters`, `--enable-preview`, `-Xlint` does
    /// not), in command-line order (a later flag can override an earlier one).
    pub compiler_flags: &'a [&'a str],
    /// The annotation-processor path entries' content digests, in order.
    pub processor_path: &'a [Digest],
    /// The `-Akey=value` options. Their order does not matter to `javac` except between
    /// repeats of one key, so the key sorts them by key, keeping repeats in order.
    pub processor_options: &'a [(&'a str, &'a str)],
}

impl EnvironmentKey {
    /// The key of `inputs`.
    pub fn derive<H: KeyHasher>(inputs: &EnvironmentInputs<'_>, hasher: H) -> Self {
        let mut encoder = Encoder::new(hasher, "jfl.environment-key.v1");
        for digests in [inputs.classpath, inputs.module_path] {
            encoder.len(digests.len());
            for digest in digests {
                encoder.digest(digest);
            }
        }
        encoder.str(inputs.jdk.vendor);
        encoder.str(inputs.jdk.version);
        encoder.digest(&inputs.jdk.release_file);
        match inputs.release {
            Some(release) => {
                encoder.u32(1);
                encoder.u32(release);
            }
            None => encoder.u32(0),
        }
        encoder.len(inputs.compiler_flags.len());
        for flag in inputs.compiler_flags {
            encoder.str(flag);
        }
        encoder.len(inputs.processor_path.len());
        for digest in inputs.processor_path {
            encoder.digest(digest);
        }
        let mut options: Vec<&(&str, &str)> = inputs.processor_options.iter().collect();
        options.sort_by_key(|(key, _)| *key);
        encoder.len(options.len());
        for (key, value) in options {
            encoder.str(key);
            encoder.str(value);
        }
        Self(encoder.finish())
    }
}

impl SemanticKey {
    /// The key of a file with `syntax`, compiled in `environment`, whose resolution read the
    /// declaration APIs whose digests are `reads` (in any order, repeats allowed).
    pub fn derive<H: KeyHasher>(
        syntax: &SyntaxKey,
        environment: &EnvironmentKey,
        reads: &[Digest],
        hasher: H,
    ) -> Self {
        let mut reads = reads.to_vec();
        reads.sort_unstable();
        reads.dedup();
        let mut encoder = Encoder::new(hasher, "jfl.semantic-key.v1");
        encoder.digest(&syntax.0);
        encoder.digest(&environment.0);
        encoder.len(reads.len());
        for read in &reads {
            encoder.digest(read);
        }
        Self(encoder.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::testing::RecordingHasher;
    use alloc::collections::BTreeSet;
    use alloc::vec;

    fn digest(n: u8) -> Digest {
        Digest::from_bytes([n; 32])
    }

    fn syntax(inputs: &SyntaxInputs<'_>) -> SyntaxKey {
        SyntaxKey::derive(inputs, RecordingHasher::default())
    }

    fn environment(inputs: &EnvironmentInputs<'_>) -> EnvironmentKey {
        EnvironmentKey::derive(inputs, RecordingHasher::default())
    }

    fn semantic(reads: &[Digest]) -> SemanticKey {
        let syntax = SyntaxKey::from_digest(digest(1));
        let environment = EnvironmentKey::from_digest(digest(2));
        SemanticKey::derive(&syntax, &environment, reads, RecordingHasher::default())
    }

    const JDK: JdkIdentity<'static> = JdkIdentity {
        vendor: "Eclipse Adoptium",
        version: "21.0.5+11",
        release_file: Digest::from_bytes([9; 32]),
    };

    #[test]
    fn should_change_each_key_when_any_of_its_inputs_changes() {
        // given
        let base = SyntaxInputs {
            file: digest(1),
            grammar: "tree-sitter-java-orchard",
            grammar_version: "0.5.2",
            summariser_version: 1,
        };
        let syntaxes = [
            base,
            SyntaxInputs {
                file: digest(2),
                ..base
            },
            SyntaxInputs {
                grammar: "tree-sitter-java",
                ..base
            },
            SyntaxInputs {
                grammar_version: "0.5.3",
                ..base
            },
            SyntaxInputs {
                summariser_version: 2,
                ..base
            },
        ];
        let env = EnvironmentInputs {
            classpath: &[digest(1), digest(2)],
            module_path: &[],
            jdk: JDK,
            release: Some(17),
            compiler_flags: &["-parameters"],
            processor_path: &[digest(3)],
            processor_options: &[("a", "1")],
        };
        let environments = [
            env,
            EnvironmentInputs {
                classpath: &[digest(2), digest(1)],
                ..env
            },
            EnvironmentInputs {
                classpath: &[digest(1)],
                module_path: &[digest(2)],
                ..env
            },
            EnvironmentInputs {
                jdk: JdkIdentity {
                    vendor: "Azul",
                    ..JDK
                },
                ..env
            },
            EnvironmentInputs {
                jdk: JdkIdentity {
                    version: "21.0.6",
                    ..JDK
                },
                ..env
            },
            EnvironmentInputs {
                jdk: JdkIdentity {
                    release_file: digest(8),
                    ..JDK
                },
                ..env
            },
            EnvironmentInputs {
                release: None,
                ..env
            },
            EnvironmentInputs {
                release: Some(21),
                ..env
            },
            EnvironmentInputs {
                compiler_flags: &[],
                ..env
            },
            EnvironmentInputs {
                processor_path: &[],
                ..env
            },
            EnvironmentInputs {
                processor_options: &[("a", "2")],
                ..env
            },
        ];
        let semantics = [
            semantic(&[digest(3)]),
            semantic(&[digest(4)]),
            semantic(&[digest(3), digest(4)]),
            semantic(&[]),
        ];
        // when
        let syntax_keys: BTreeSet<_> = syntaxes.iter().map(syntax).collect();
        let environment_keys: BTreeSet<_> = environments.iter().map(environment).collect();
        let semantic_keys: BTreeSet<_> = semantics.into_iter().collect();
        // then
        assert_eq!(syntax_keys.len(), syntaxes.len());
        assert_eq!(environment_keys.len(), environments.len());
        assert_eq!(semantic_keys.len(), 4);
        let other_env = SemanticKey::derive(
            &SyntaxKey::from_digest(digest(1)),
            &EnvironmentKey::from_digest(digest(7)),
            &[digest(3)],
            RecordingHasher::default(),
        );
        assert_ne!(other_env, semantic(&[digest(3)]));
    }

    #[test]
    fn should_derive_one_semantic_key_when_reads_are_reordered_or_repeated() {
        // given
        let reads = vec![digest(5), digest(3), digest(4)];
        // when
        let keys = [
            semantic(&reads),
            semantic(&[digest(3), digest(4), digest(5)]),
            semantic(&[digest(4), digest(5), digest(3), digest(5), digest(4)]),
        ];
        // then
        assert!(keys.iter().all(|key| *key == keys[0]));
    }

    #[test]
    fn should_derive_one_environment_key_when_processor_options_are_reordered() {
        // given
        let env = |options| EnvironmentInputs {
            classpath: &[],
            module_path: &[],
            jdk: JDK,
            release: None,
            compiler_flags: &[],
            processor_path: &[],
            processor_options: options,
        };
        // when
        let sorted = environment(&env(&[("a", "1"), ("b", "2")]));
        let reordered = environment(&env(&[("b", "2"), ("a", "1")]));
        let repeats = environment(&env(&[("a", "1"), ("a", "2")]));
        let repeats_swapped = environment(&env(&[("a", "2"), ("a", "1")]));
        // then
        assert_eq!(sorted, reordered);
        assert_ne!(
            repeats, repeats_swapped,
            "the last repeat wins, so their order counts"
        );
    }

    #[test]
    fn should_never_collide_when_two_kinds_of_key_share_inputs() {
        // given
        // inputs that are all zeros and empty strings
        // when
        let empty_syntax = syntax(&SyntaxInputs {
            file: digest(0),
            grammar: "",
            grammar_version: "",
            summariser_version: 0,
        });
        let empty_semantic = SemanticKey::derive(
            &SyntaxKey::from_digest(digest(0)),
            &EnvironmentKey::from_digest(digest(0)),
            &[],
            RecordingHasher::default(),
        );
        // then
        assert_ne!(empty_syntax.digest(), empty_semantic.digest());
    }
}
