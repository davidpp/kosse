# Implementation loop

This is a harness-neutral runbook, not a background scheduler. Nothing in this planning setup starts a loop automatically.

## Invocation prompt

> Implement Stackscan epic JSTA-1 from this repository. Read AGENTS.md and blueprint/README.md. Use Kabane scope jake://scope/stackscan and process dependency-ready child issues in blueprint order. Follow blueprint/loop.md. Implement, verify through actual entrypoints, record evidence, and continue until complete or blocked. Do not publish, push, change login startup, stop real services, or claim unperformed GUI checks. Do not commit unless separately authorized.

## Each cycle

1. Read `git status --short`, project instructions, and `kabane context JSTA-1`. List `kabane list --scope jake://scope/stackscan --kind issue`.
2. Select the earliest non-done child whose blockers are done. Read its full `kabane context <id>` and relevant blueprint sections. Verify prerequisite artifacts exist; tracker completion alone is not proof.
3. Mark that child in_progress. If resuming, read comments/logs and inspect the actual diff. Do not wipe WIP or restart from an assumed clean tree.
4. Implement only that slice. Use current docs/generators, lock versions, write tests with behavior. Small inspectable increments are preferred; no approval pause per diff is needed inside an explicitly invoked loop.
5. Run targeted tests plus applicable aggregate gates. Demonstrate the user-facing path described in the issue. Do not replace failing assertions with weaker ones to get green.
6. Record files, command outcomes, evidence and remaining gaps in Kabane (`comment` and `log`). Mark done only if acceptance holds. The epic remains open until the final audit.
7. Continue to next ready child. No hidden runtime-specific state file is needed; harness checkpoints stay with the harness, task state in Kabane, code in git.
8. On completion, summarize installation/use commands and known limits; do not automatically release.

## Stop conditions

After 2–3 failed approaches to the same blocker, stop and record the diagnosis. Also stop for ambiguous destructive scope, unsupported library behavior requiring a material architecture change, missing credentials/permissions, real-machine startup changes, source licensing uncertainty, or required human GUI verification.

Mark blocked work waiting with a concrete next action and evidence; keep dependency links. Do not mark the epic done while required acceptance remains blocked. Independent ready issues may continue; avoid competing edits in one working tree.

## Authority boundaries

An implementation loop may run local generators, install project dependencies, edit this repo, execute tests, create sandbox release artifacts, and stop only its own verified fixtures. It does not authorize remote repository creation, GitHub Actions execution/publishing, code signing, public licensing decisions, system-wide installation, changes to global agent policies, or killing existing user services.

If the implementation must change a blueprint contract, record the reason and update the smallest relevant section. Minor library substitutions are allowed when acceptance and preferences remain intact; material scope/security changes require David's decision.
