# Code Review: Blind Hunter (No Context)

**Role:** Find logic errors, security issues, and code quality problems **without** project context or spec knowledge.

**Input:** Diff only (Story 8-3: User administration)

**Task:** Review the diff below for:
- Logic bugs (null checks, error handling, type safety)
- Security vulnerabilities (auth bypass, injection, privilege escalation, data exposure)
- Race conditions (concurrent access, atomicity violations)
- Code quality issues (DRY violations, naming, comments)
- Off-by-one errors, boundary conditions

**Output format:**
```
## Finding: [Title]
- **Severity**: Critical | High | Medium | Low
- **Location**: src/file.rs:line
- **Evidence**: [excerpt from diff]
- **Issue**: [What's wrong]
- **Impact**: [Why it matters]
```

---

## DIFF

```
[Story 8-3 User Administration full diff - see attached file or paste below]
```

**Review the diff carefully and output findings above.**
