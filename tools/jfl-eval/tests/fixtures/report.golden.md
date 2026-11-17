# E0 report

Harbor 0.26.0 · report schema 1 · 6 runs read · 1 skipped (task not written by jfl-eval) · 1 unreadable

Data quality: 2 malformed lines, 0 oversized lines, 0 cut files, 0 duplicate lines, 0 runs without a transcript.

## Arm `grep-only`

3 runs · 2 successes · 0 without a verdict · success rate 0.67 (a run without a verdict counts as a failure; 0.67 over the runs with one) · transcripts: session 3

| Metric (per run) | n | Mean | SD | Min | Max |
|:--|--:|--:|--:|--:|--:|
| builds | 3 | 1.00 | 1.00 | 0.00 | 2.00 |
| compile_failures_with_diagnostic_before_fix | 3 | 0.00 | 0.00 | 0.00 | 0.00 |
| diagnostics_events | 3 | 0.00 | 0.00 | 0.00 | 0.00 |
| edits | 3 | 1.33 | 0.58 | 1.00 | 2.00 |
| edits_then_compile_failure | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| edits_with_error_diagnostic | 3 | 0.00 | 0.00 | 0.00 | 0.00 |
| empty_references | 3 | 0.00 | 0.00 | 0.00 | 0.00 |
| lsp_calls | 3 | 0.00 | 0.00 | 0.00 | 0.00 |
| lsp_errors | 3 | 0.00 | 0.00 | 0.00 | 0.00 |
| success | 3 | 0.67 | 0.58 | 0.00 | 1.00 |
| time_to_first_lsp_answer_ms | 0 | - | - | - | - |
| tokens_cached | 3 | 1200.00 | 519.62 | 900.00 | 1800.00 |
| tokens_input | 3 | 1409.67 | 612.57 | 1056.00 | 2117.00 |
| tokens_output | 3 | 103.33 | 49.07 | 75.00 | 160.00 |
| tokens_total | 3 | 1513.00 | 661.64 | 1131.00 | 2277.00 |
| tool_calls | 3 | 4.00 | 1.73 | 3.00 | 6.00 |

| Per successful run | n | Mean | SD | Min | Max |
|:--|--:|--:|--:|--:|--:|
| tokens_total | 2 | 1704.00 | 810.34 | 1131.00 | 2277.00 |
| tool_calls | 2 | 4.50 | 2.12 | 3.00 | 6.00 |

| Tool calls by kind (per run) | n | Mean | SD | Min | Max |
|:--|--:|--:|--:|--:|--:|
| bash | 3 | 1.00 | 1.00 | 0.00 | 2.00 |
| edit | 3 | 1.33 | 0.58 | 1.00 | 2.00 |
| glob | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| grep | 3 | 1.00 | 0.00 | 1.00 | 1.00 |
| read | 3 | 0.33 | 0.58 | 0.00 | 1.00 |

| LSP operation (per run) | n | Mean | SD | Min | Max |
|:--|--:|--:|--:|--:|--:|
| (none) | 0 | - | - | - | - |

| Next action after an empty findReferences | Count |
|:--|--:|
| (none) | 0 |

| Task | Runs | Successes | Success rate | Tool calls | Tokens | Edits then compile failure | LSP calls |
|:--|--:|--:|--:|--:|--:|--:|--:|
| fix-rounding | 2 | 1 | 0.50 | 4.50 ± 2.12 | 1704.00 ± 810.34 | 0.50 ± 0.71 | 0.00 ± 0.00 |
| rename-entry | 1 | 1 | 1.00 | 3.00 ± 0.00 | 1131.00 ± 0.00 | 0.00 ± 0.00 | 0.00 ± 0.00 |

## Arm `jdtls`

3 runs · 1 successes · 1 without a verdict · success rate 0.33 (a run without a verdict counts as a failure; 0.50 over the runs with one) · transcripts: atif 1, session 2

| Metric (per run) | n | Mean | SD | Min | Max |
|:--|--:|--:|--:|--:|--:|
| builds | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| compile_failures_with_diagnostic_before_fix | 2 | 0.50 | 0.71 | 0.00 | 1.00 |
| diagnostics_events | 2 | 1.00 | 1.41 | 0.00 | 2.00 |
| edits | 3 | 1.00 | 1.00 | 0.00 | 2.00 |
| edits_then_compile_failure | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| edits_with_error_diagnostic | 2 | 0.50 | 0.71 | 0.00 | 1.00 |
| empty_references | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| lsp_calls | 3 | 2.33 | 2.31 | 1.00 | 5.00 |
| lsp_errors | 3 | 0.67 | 1.15 | 0.00 | 2.00 |
| success | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| time_to_first_lsp_answer_ms | 3 | 5666.67 | 2081.67 | 4000.00 | 8000.00 |
| tokens_cached | 3 | 2333.33 | 1761.63 | 300.00 | 3400.00 |
| tokens_input | 3 | 2774.67 | 2101.19 | 352.00 | 4100.00 |
| tokens_output | 3 | 140.00 | 126.19 | 25.00 | 275.00 |
| tokens_total | 3 | 2914.67 | 2197.99 | 377.00 | 4220.00 |
| tool_calls | 3 | 4.67 | 5.51 | 1.00 | 11.00 |

| Per successful run | n | Mean | SD | Min | Max |
|:--|--:|--:|--:|--:|--:|
| tokens_total | 1 | 4147.00 | 0.00 | 4147.00 | 4147.00 |
| tool_calls | 1 | 11.00 | 0.00 | 11.00 | 11.00 |

| Tool calls by kind (per run) | n | Mean | SD | Min | Max |
|:--|--:|--:|--:|--:|--:|
| agent | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| bash | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| edit | 3 | 1.00 | 1.00 | 0.00 | 2.00 |
| grep | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| lsp | 3 | 2.33 | 2.31 | 1.00 | 5.00 |
| mcp | 3 | 0.33 | 0.58 | 0.00 | 1.00 |

| LSP operation (per run) | n | Mean | SD | Min | Max |
|:--|--:|--:|--:|--:|--:|
| findReferences | 3 | 0.67 | 0.58 | 0.00 | 1.00 |
| goToDefinition | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| hover | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| other | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| prepareCallHierarchy | 3 | 0.33 | 0.58 | 0.00 | 1.00 |
| workspaceSymbol | 3 | 0.33 | 0.58 | 0.00 | 1.00 |

| Next action after an empty findReferences | Count |
|:--|--:|
| grep | 1 |

| Task | Runs | Successes | Success rate | Tool calls | Tokens | Edits then compile failure | LSP calls |
|:--|--:|--:|--:|--:|--:|--:|--:|
| fix-rounding | 2 | 1 | 0.50 | 6.50 ± 6.36 | 4183.50 ± 51.62 | 0.50 ± 0.71 | 3.00 ± 2.83 |
| rename-entry | 1 | 0 | 0.00 | 1.00 ± 0.00 | 377.00 ± 0.00 | 0.00 ± 0.00 | 1.00 ± 0.00 |
