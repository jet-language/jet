# Process-session native fixture closeout

Criterion 6 requires native fixture evidence for every supported platform.
Decision `D-PLATFORM-EVIDENCE1 = D` requires a matching CI pass for each
native platform row. A generic host job does not close a row.

## Capability map

The checked-in matrix is `tests/fixtures/process_sessions/compatibility.tsv`.
It maps every capability to `session_fixture.rs` and to one native integration
test:

| Capability | Fixture | Test |
| --- | --- | --- |
| `terminal-byte-stream` | `session_fixture.rs` | `native_fixture_terminal_bytes_resize_and_closed_stream` |
| `terminal-resize` | `session_fixture.rs` | `native_fixture_terminal_bytes_resize_and_closed_stream` |
| `terminal-input-and-close` | `session_fixture.rs` | `native_fixture_terminal_bytes_resize_and_closed_stream` |
| `process-tree-interrupt` | `session_fixture.rs` | `native_fixture_controls_the_full_tree_on_interrupt_timeout_and_drop` |
| `process-tree-terminate` | `session_fixture.rs` | `native_fixture_controls_the_full_tree_on_interrupt_timeout_and_drop` |
| `process-tree-kill` | `session_fixture.rs` | `native_fixture_controls_the_full_tree_on_interrupt_timeout_and_drop` |
| `process-tree-timeout` | `session_fixture.rs` | `native_fixture_controls_the_full_tree_on_interrupt_timeout_and_drop` |
| `process-tree-drop` | `session_fixture.rs` | `native_fixture_controls_the_full_tree_on_interrupt_timeout_and_drop` |

Linux passed both native fixture tests on this host. The matrix parser also
passed. These local results do not close a native row under the decision.

## Platform rows

| Platform row | Status | Evidence or remaining action |
| --- | --- | --- |
| Linux x86_64 | NOT CLOSED | Local native fixture tests pass. No matching CI job runs `tests/process_sessions.rs`. |
| macOS | NOT CLOSED | No matching CI pass runs the native fixture. |
| Windows | NOT CLOSED | No matching CI pass runs the ConPTY fixture. The existing Windows matrix job runs other tests only. |
| Other targets | CLOSED | `compatibility.tsv` records `unsupported` for every capability. |

The existing `jetpack-platform` matrix in `.github/workflows/ci.yml:54-88`
does not run `tests/process_sessions.rs`. The Ubuntu-only `verify-tests`
matrix at `.github/workflows/ci.yml:211-235` is a generic host job and cannot
close macOS or Windows rows. No owner-ratified exclusion names Linux, macOS,
or Windows.

## CI job required

Add this job under `jobs:` in `.github/workflows/ci.yml`. The file is not
edited under the card rules.

```yaml
  process-session-native-fixtures:
    name: Process-session native fixtures (${{ matrix.label }})
    if: github.event_name != 'schedule'
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-latest
            label: linux-x86_64
          - os: macos-latest
            label: macos
          - os: windows-latest
            label: windows
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Native process-session fixture proof
        run: |
          cargo test --test process_sessions native_fixture_ -- --nocapture
          cargo test --test process_sessions compatibility_matrix_names_every_session_capability -- --nocapture
```

One passing run of this job closes the matching Linux, macOS, and Windows
rows. Until that run exists, criterion 6 is not met.

## Local test evidence

```text
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 13 filtered out; finished in 17.65s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 14 filtered out; finished in 0.00s

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| `PROCESS-SESSION-CI-MATRIX` | card | #1186 |
| `PLATFORM-EVIDENCE-LAW` | decision | D-PLATFORM-EVIDENCE1=D |
| `UNSUPPORTED-TARGETS` | no-action | no-action: the compatibility matrix records unsupported targets explicitly |
<!-- /audit-dispositions -->
```
