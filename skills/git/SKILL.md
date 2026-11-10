---
name: git
description: Load before creating commits, branches, tags, or recovering Git working tree/index state. Enforces explicit per-action user approval before any push or history change. Do NOT load for read-only `git status` or `git diff` inspections when no Git write is requested.
---

# Git Safety Skill

## Core Invariants
1. **Never Commit or Push Without Explicit Ask:** Only create commits when explicitly asked by the user in the current session.
2. **Never Force Rewrite History:** No `reset --hard`, `rebase -i`, or destructive operations on published history.
3. **Stage Explicitly by Name:** Never use broad git add (`git add .`) without verifying untracked user files.
4. **Clean Commit Messages:** Use conventional commit messages (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`).
