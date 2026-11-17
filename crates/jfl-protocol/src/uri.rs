//! A document URI as a peer wrote it, beside the [`FileUri`] it names.
//!
//! LSP clients compare URIs as strings, so a server that answers about a document the client named
//! must echo the client's own spelling (`file:///a%2Bb/A.java` and `file:///a+b/A.java` name one
//! file, but a client that sent one does not match the other; LSP-01's decision). A
//! [`DocumentUri`] therefore keeps the text exactly as received and the parsed [`FileUri`] used for
//! every lookup; it is written back as the text. A URI the daemon makes itself carries
//! [`FileUri`]'s canonical form.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use core::fmt;

use jfl_domain::FileUri;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A `file://` URI: the text as sent and the file it names.
///
/// Equality, ordering and hashing cover the text too, so two spellings of one file differ: a map
/// of documents (overlays, open files) keys on [`DocumentUri::file`], never on the
/// `DocumentUri`, or a `didClose` spelled otherwise than its `didOpen` would leak the entry.
///
/// Pinned by: `uri::tests::should_echo_the_text_as_sent_when_a_uri_is_decoded` and
/// `uri::tests::should_refuse_a_uri_when_it_is_not_a_file_uri`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentUri {
    text: Box<str>,
    file: FileUri,
}

impl DocumentUri {
    /// Parses `text` (`FileUri::parse`) and keeps it as sent.
    pub fn parse(text: &str) -> Result<Self, jfl_domain::UriError> {
        let file = FileUri::parse(text)?;
        Ok(Self {
            text: text.into(),
            file,
        })
    }

    /// The URI's text as sent, or the canonical form when the daemon made it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The file it names.
    #[must_use]
    pub const fn file(&self) -> &FileUri {
        &self.file
    }
}

/// The canonical text of `file`.
impl From<FileUri> for DocumentUri {
    fn from(file: FileUri) -> Self {
        Self {
            text: file.to_string().into_boxed_str(),
            file,
        }
    }
}

impl From<&FileUri> for DocumentUri {
    fn from(file: &FileUri) -> Self {
        Self::from(file.clone())
    }
}

impl fmt::Display for DocumentUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Serialize for DocumentUri {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for DocumentUri {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UriVisitor;

        impl Visitor<'_> for UriVisitor {
            type Value = DocumentUri;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a file URI")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<DocumentUri, E> {
                DocumentUri::parse(text).map_err(E::custom)
            }

            fn visit_string<E: de::Error>(self, text: String) -> Result<DocumentUri, E> {
                let file = FileUri::parse(&text).map_err(E::custom)?;
                Ok(DocumentUri {
                    text: text.into_boxed_str(),
                    file,
                })
            }
        }

        deserializer.deserialize_str(UriVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_echo_the_text_as_sent_when_a_uri_is_decoded() {
        // given
        let sent = "\"file:///repo/a+b/A%2Ejava\"";

        // when
        let uri: DocumentUri = serde_json::from_str(sent).unwrap();

        // then
        assert_eq!(uri.as_str(), "file:///repo/a+b/A%2Ejava");
        assert_eq!(uri.file().path(), "/repo/a+b/A.java");
        assert_eq!(serde_json::to_string(&uri).unwrap(), sent);
    }

    #[test]
    fn should_print_the_canonical_form_when_the_uri_comes_from_a_file() {
        // given
        let file = FileUri::from_path("/repo/a+b/A.java").unwrap();

        // when
        let uri = DocumentUri::from(&file);

        // then
        assert_eq!(uri.as_str(), "file:///repo/a%2Bb/A.java");
        assert_eq!(uri.file(), &file);
    }

    #[test]
    fn should_refuse_a_uri_when_it_is_not_a_file_uri() {
        // given
        let sent = ["\"https://example.com/A.java\"", "\"file:///a/../b\"", "7"];

        for text in sent {
            // when
            let uri = serde_json::from_str::<DocumentUri>(text);

            // then
            assert!(uri.is_err(), "{text}");
        }
    }
}
