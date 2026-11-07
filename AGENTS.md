# Technical Decision & Documentation Policy

# Project workflow

- Automatically create a Git commit when a milestone or ticket is complete, after running the relevant checks. No additional confirmation is needed.
- Keep each commit focused on the completed milestone or ticket, with a clear message describing the change.
- Stage only changes belonging to the completed work; do not include unrelated user changes.
- If checks fail or the work is incomplete, report the blocker rather than committing it as completed work.
- This authorization covers local commits only. Push only when the user requests it.

## 1. Core Principles

- Prefer modern, stable, officially recommended technologies and APIs.
- Never assume that a technology, API, dependency, or architectural pattern is still recommended based solely on training knowledge.
- Never introduce deprecated, legacy, experimental, or unmaintained solutions into new code without a documented justification.
- Respect the project's existing architecture and supported versions. Do not modernize unrelated code without explicit justification.
- Prefer simplicity, maintainability, security, and compatibility over novelty.
- Do not equate "latest" with "best": prefer actively supported stable versions suitable for the project.

## 2. Mandatory Research Triggers

Before making a NEW technical decision, consult up-to-date documentation when the task involves:

- Selecting a framework, library, dependency, or runtime.
- Choosing an architecture or infrastructure component.
- Introducing a new API or integration pattern.
- Configuring deployment, authentication, messaging, persistence, or security.
- Selecting between competing approaches with significant maintenance implications.
- Using a feature whose stability, deprecation status, or recommended usage is uncertain.

Research is NOT required for:
- Routine implementation using already approved technologies.
- Small bug fixes unrelated to technical decisions.
- Refactoring that preserves established architecture.
- Previously validated decisions whose assumptions remain applicable.

## 3. Research Protocol

When research is required:

1. Check `docs/tech-stack.md` for an existing validated decision.
2. Inspect project manifests, lockfiles, configuration, and existing code to determine actual versions and constraints.
3. If the decision is not already covered, consult current official documentation.
4. Check deprecations, migration guides, release notes, and compatibility requirements.
5. Prefer stable, maintained, officially recommended solutions.
6. Record the decision before implementing it.

Source priority:
1. Official documentation for the relevant version.
2. Official release notes and migration guides.
3. Maintainer-maintained repositories.
4. Other reliable technical references when official sources are insufficient.

Do not treat tutorials, Stack Overflow posts, or old examples as authoritative without verification.

Never claim a choice was verified against current documentation if no such verification occurred. If browsing or documentation tools are unavailable, explicitly report the limitation.

## 4. Technical Decision Memory

Maintain `docs/tech-stack.md` as the project's persistent technical decision register.

For each meaningful new decision, record:
- Technology or component.
- Selected version or supported version range.
- Selected approach.
- Rejected deprecated or legacy alternatives, if relevant.
- Brief rationale.
- Official documentation URL.
- Verification date.

Before researching a previously encountered technology, consult this register.

Reuse an existing decision when:
- The project context has not materially changed.
- The selected version remains applicable.
- No relevant breaking change or deprecation is indicated.
- The decision still satisfies the current requirements.

Revalidate decisions when:
- A major version changes.
- A deprecation or compatibility warning appears.
- A new requirement invalidates the original assumptions.
- A dependency or security issue requires reconsideration.

Never repeatedly research an unchanged and already validated decision.

## 5. Legacy Technology Prevention

For new components:
- Prefer current officially recommended APIs and architectures.
- Do not copy legacy infrastructure from old tutorials.
- Avoid compatibility shims and deprecated packages unless required.
- Avoid adding dependencies already superseded by native framework functionality.

For existing components:
- Preserve compatibility unless modernization is in scope.
- Identify legacy technology rather than silently expanding its usage.
- Do not introduce a migration merely because a newer version exists.

## 6. Token Efficiency

- Read the technical decision register before using external research.
- Research only the unresolved technical question.
- Prefer targeted documentation sections over entire documentation sites.
- Do not repeatedly fetch the same references during a task.
- Summarize findings into concise, persistent decisions.
- Avoid copying full documentation pages into the repository or conversation.
- Do not rewrite existing decisions unless new evidence changes them.
- Keep the decision register concise and remove redundant explanations.

## 7. Implementation Validation

Before completing a significant technical change:
- Verify that the implementation matches the approved technical decisions.
- Check for newly introduced deprecated APIs or unnecessary legacy dependencies.
- Confirm compatibility with actual project versions.
- Run relevant tests, builds, and static checks when available.
- Report any deviation from the approved stack.

## 8. Continuous Improvement

When an outdated or inappropriate technical choice is discovered:
1. Correct the implementation if within task scope.
2. Determine why the outdated choice was selected.
3. Update `docs/tech-stack.md` with the validated replacement.
4. Add a concise rule to the project's `AGENTS.md` only if the mistake represents a recurring generalizable pattern.
5. Avoid duplicating technology-specific decisions across instruction files.

The goal is to prevent recurrence without expanding permanent instructions unnecessarily.