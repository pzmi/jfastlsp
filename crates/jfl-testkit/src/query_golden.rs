//! Query goldens (ver §4; IDX-10, completed for M1's 50 cases in LSP-06): a query's answer
//! rendered through `jfl-protocol`'s wire types (the envelope as `jfl/definition` and
//! `jfl/references` carry it, the symbols as `jfl/symbols` does), pretty-printed so a review
//! reads a diff line by line, and compared byte for byte with a committed file.
//!
//! **Goldens are never rewritten to get green.** [`check`] fails on any difference; a case with
//! no golden gets one written and fails too ("review and commit it"), so a new case's golden is
//! reviewed before it passes. A deliberate change deletes the golden, reruns, reviews the new file
//! and moves its line in `fixtures.lock` (`just fixtures-lock`).
//!
//! Used by `jfl-application`'s `tests/queries.rs`.

use std::path::Path;

use jfl_application::Symbols;
use jfl_domain::{Envelope, FileUri, Position};
use jfl_protocol::envelope::{Operation, Query, WireLocation, WireRange, WireView};
use jfl_protocol::messages::{SymbolKind, SymbolsResult, WireSymbol};
use jfl_protocol::{ClaimedCompleteness, DocumentUri, SCHEMA_VERSION, WireEnvelope};
fn pretty(encoded: serde_json::Result<String>) -> Result<String, String> {
    encoded
        .map(|mut text| {
            text.push('\n');
            text
        })
        .map_err(|e| e.to_string())
}

/// The `jfl/definition` result of `envelope`, asked at `position` of `uri`.
pub fn definition(
    envelope: &Envelope,
    uri: &FileUri,
    position: Position,
) -> Result<String, String> {
    let query = Query {
        op: Operation::Def,
        uri: DocumentUri::from(uri),
        line: position.line(),
        col: position.column(),
    };
    pretty(serde_json::to_string_pretty(&WireEnvelope::from_domain(
        envelope,
        Some(query),
    )))
}

/// The `jfl/references` result of `envelope`, asked at `position` of `uri`.
pub fn references(
    envelope: &Envelope,
    uri: &FileUri,
    position: Position,
) -> Result<String, String> {
    let query = Query {
        op: Operation::Refs,
        uri: DocumentUri::from(uri),
        line: position.line(),
        col: position.column(),
    };
    pretty(serde_json::to_string_pretty(&WireEnvelope::from_domain(
        envelope,
        Some(query),
    )))
}

/// The `jfl/symbols` result of `symbols`.
pub fn symbols(symbols: &Symbols) -> Result<String, String> {
    let result = SymbolsResult {
        schema_version: SCHEMA_VERSION,
        position_encoding: symbols.position_encoding,
        view: WireView::from(&symbols.view),
        symbols: symbols
            .symbols
            .iter()
            .map(|s| WireSymbol {
                symbol: s
                    .symbol
                    .as_ref()
                    .map_or_else(|| s.name.clone(), ToString::to_string),
                kind: SymbolKind::from(s.kind),
                location: WireLocation {
                    uri: DocumentUri::from(&s.location.uri),
                    range: WireRange(s.location.range),
                },
            })
            .collect(),
        completeness: ClaimedCompleteness::from(&symbols.completeness),
    };
    pretty(serde_json::to_string_pretty(&result))
}

/// Compares `got` with the golden `dir/<name>.json`; writes the golden when there is none (and
/// fails, so it is reviewed before it passes).
pub fn check(dir: &Path, name: &str, got: &str) -> Result<(), String> {
    let path = dir.join(format!("{name}.json"));
    match std::fs::read_to_string(&path) {
        Ok(want) if want == got => Ok(()),
        Ok(want) => Err(format!(
            "{name}: the answer differs from its golden {}\n--- golden\n{want}\n--- answer\n{got}",
            path.display()
        )),
        Err(_) => {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            std::fs::write(&path, got).map_err(|e| e.to_string())?;
            Err(format!(
                "{name}: no golden: wrote {}; review it, then commit it with its fixtures.lock line",
                path.display()
            ))
        }
    }
}
