
# Kova — Project Engineering Policy

## 1. Scope and Priorities

This file defines project-specific engineering rules.

Global agent orchestration, delegation, and general
collaboration rules are defined in ~/.codex/AGENTS.md.

Priority order:
1. Explicit user requirements.
2. Project architecture and established conventions.
3. Validated decisions in docs/tech-stack.md.
4. Current official documentation.
5. General engineering best practices.

If instructions conflict, report the conflict rather
than silently overriding a project constraint.

## 2. Engineering Principles

- Prefer modern, stable, maintained technologies.
- Favor simplicity, security, performance, and maintainability.
- Avoid unnecessary dependencies and abstractions.
- Respect existing architecture and supported versions.
- Do not introduce deprecated or legacy solutions into new code.
- Do not modernize unrelated components without justification.
- Prefer established project conventions over personal preferences.
- Avoid speculative implementations and premature optimization.
- Keep interfaces explicit and components loosely coupled.
- Preserve compatibility unless a breaking change is approved.

## 3. Technical Decision Policy

Never assume that a technology, API, dependency, or
architectural pattern remains recommended based solely
on training knowledge.

Research is mandatory when:
- Selecting a new framework, library, or runtime.
- Introducing an architectural or infrastructure component.
- Choosing authentication, messaging, or persistence patterns.
- Introducing unfamiliar or version-sensitive APIs.
- Making decisions with significant maintenance implications.
- Encountering uncertain deprecation or compatibility status.

Research is unnecessary for:
- Routine implementation using validated technologies.
- Small bug fixes.
- Refactoring within established architecture.
- Decisions already validated and still applicable.

### Research Protocol

1. Read docs/tech-stack.md.
2. Inspect relevant manifests, lockfiles, and existing code.
3. Reuse validated decisions when still applicable.
4. Otherwise consult current official documentation.
5. Verify compatibility, stability, and deprecation status.
6. Record meaningful new decisions before implementation.

Source priority:
1. Official version-specific documentation.
2. Official release notes and migration guides.
3. Maintainer-maintained repositories.
4. Other reliable technical sources.

Never claim a decision was verified without actual verification.

If documentation tools are unavailable, report the
limitation instead of inventing confirmation.

## 4. Technical Decision Memory

Maintain docs/tech-stack.md as the persistent technical
decision register.

For each meaningful new decision, record:
- Component or technology.
- Selected version or supported range.
- Selected approach.
- Rejected legacy alternatives, when relevant.
- Concise rationale.
- Official documentation URL.
- Last verification date.

Reuse existing decisions unless:
- A major version changes.
- A relevant deprecation is discovered.
- Requirements materially change.
- Compatibility or security issues arise.
- New evidence invalidates the decision.

Never repeatedly research unchanged validated decisions.

Keep the register concise and actionable.

## 5. Implementation Rules

- Follow established project conventions.
- Prefer existing dependencies and native framework APIs.
- Avoid introducing deprecated packages or compatibility shims.
- Keep changes limited to the requested scope.
- Do not introduce unrelated architectural changes.
- Write readable, maintainable, testable code.
- Handle errors explicitly and consistently.
- Never hardcode credentials or sensitive configuration.
- Preserve public interfaces unless changes are intentional.
- Update affected documentation when behavior changes.
- Avoid unnecessary comments that merely restate code.

### Parallel Implementation

When multiple agents implement tasks concurrently:
- Define file ownership and shared interfaces first.
- Avoid overlapping write scopes.
- Preserve compatibility with concurrent work.
- Do not modify another agent's assigned files.
- Report integration dependencies to the coordinator.
- Never treat another agent's unverified changes as stable.

## 6. Validation and Quality

Before declaring a task complete:
1. Review the actual code changes.
2. Verify compliance with docs/tech-stack.md.
3. Check for deprecated APIs or unnecessary dependencies.
4. Confirm compatibility with project versions.
5. Run relevant unit tests and integration tests.
6. Run relevant builds and static checks.
7. Verify affected interfaces and integration points.
8. Report any remaining failures or limitations.

For multi-agent work, the coordinating agent is
responsible for final integration and validation.

Do not claim tests passed if they were not executed.

Do not mark incomplete or failing work as complete.

## 7. Git Workflow

Automatically create a local Git commit when a milestone
or ticket is complete and relevant checks have passed.

No additional confirmation is required for local commits.

Rules:
- Keep commits focused on one completed milestone or ticket.
- Use clear, descriptive commit messages.
- Stage only files belonging to the completed work.
- Never include unrelated user changes.
- Never overwrite or discard uncommitted user changes.
- Never push unless explicitly requested.
- If validation fails, report the blocker instead of
  committing the work as completed.

### Multi-Agent Git Ownership

Only the main coordinating agent may:
- Stage changes.
- Create commits.
- Rebase or reset project history.
- Perform final integration.

Subagents must never:
- Create commits.
- Push changes.
- Modify the Git index.
- Reset or rebase branches.
- Discard changes created by other agents.

The main agent must wait for relevant subagents,
review their changes, resolve integration issues,
run final checks, and create the milestone commit.

## 8. Context and Token Efficiency

- Inspect only files relevant to the task.
- Check the decision register before researching.
- Research unresolved technical questions only.
- Prefer targeted documentation sections.
- Avoid repeatedly fetching the same references.
- Avoid unnecessary repository-wide scans.
- Do not duplicate decisions across documentation files.
- Keep reports concise and focused on actionable results.
- Avoid generating documentation that provides no lasting value.
- Do not repeat investigations already completed in the task.

Never sacrifice correctness or verification merely
to reduce token consumption.

## 9. Continuous Improvement

When an outdated or inappropriate technical choice
is discovered:

1. Correct it if within task scope.
2. Identify the cause of the incorrect decision.
3. Update docs/tech-stack.md with the validated approach.
4. Update this AGENTS.md only if the issue reveals a
   recurring, generalizable project-level problem.
5. Avoid adding redundant or technology-specific rules.

The objective is to prevent recurring errors without
continuously expanding permanent instructions.

## 10. Completion Criteria

A milestone is complete only when:
- Requested functionality is implemented.
- Relevant tests and builds pass.
- Integration is verified.
- Project technical decisions are respected.
- Required documentation is updated.
- No known blocking issues remain.
- A focused local commit has been created.

The final report must summarize:
- Implemented changes.
- Validation performed and its results.
- Commit identifier, if created.
- Remaining limitations, if any.
