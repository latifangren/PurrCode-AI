---
name: review
description: Code review and verification checklist for bugs, edge cases, and architectural regressions.
---

# Review Skill

Use this skill when auditing changes, investigating bug reports, or reviewing PR deliverables.

## Checklist
1. **Contract Derivation:** Derive the actual contract from existing tests and call sites, not just the issue summary.
2. **Boundary Testing:** Check boundary values (0, empty string, max capacity, EOF, network timeout).
3. **No Weakening:** Never weaken an existing test or skip checks to force code to pass.
4. **Verification Evidence:** Do not claim a fix is complete without running the real test command and observing exit code 0.
