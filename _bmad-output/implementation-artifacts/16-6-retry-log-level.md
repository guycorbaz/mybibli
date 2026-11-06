# Story 16.6: Metadata retry log level during backups

Status: done

GitHub issue: #637 (type:bug). Covers: FR171. Epic 16; ships in v1.26.0.

## Story

As an operator,
I want an expected nightly database outage not to read as an error,
so that an `ERROR` line means "look at this".

## Acceptance Criteria

1. **AC1.** A metadata retry tick that fails on a pool timeout logs at `WARN`; every other failure stays at `ERROR`; the retry behaviour does not change.
2. **AC2.** A unit test covers the choice (`only_a_pool_timeout_reads_as_an_expected_outage`).

## Notes

The project lead confirmed on 2026-11-06 that the nightly window matches the NAS backups. No E2E: the outage cannot be reproduced through a user journey, and no user-visible behaviour changes.
