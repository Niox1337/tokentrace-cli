# Adapt TokenTrace Workflow

Do not credit AI (Claude) in commit messages, issues, or pull request descriptions.

## Complete Workflow

Every development cycle should follow this order:

1. Create a GitHub issue with the corresponding labels and milestone.
2. Create a branch for that issue.
3. Implement the work through multiple commits.
4. Push the branch to the remote.
5. Create a pull request that closes the issue automatically when merged.
6. Merge the pull request and delete the branch.

The pull request body must include:

```text
Closes #[issue_number]
- Keep the commit format:
  ```text
  [type](one_word_summary) #[issue_number]: [message_body]
  ```
- Keep allowed commit types:
  ```text
  feat
  docs
  fix
  chore
  ```
- Replace area labels with TokenTrace labels:
  ```text
  cli
  tui
  core
  adapters
  claude-code
  storage
  git
  metrics
  privacy
  docs
  ci
  release
  ```

- Replace Python checks with Rust checks:
  ```bash
  cargo fmt --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo build --workspace
  ```
- Replace examples with TokenTrace examples, such as:
  ```text
  feat(cli) #12: add adapter listing command
  feat(storage) #12: create initial sqlite schema
  feat(tui) #12: render session overview screen
  bug(metrics) #12: keep estimated tokens separate from measured totals
  docs(privacy) #12: document sensitive import defaults
  chore(ci): add cargo fmt check
  ```

## Adaptive Commit Time Gap Rule

Each commit should be separated by enough time for a human coder to reasonably write the same change, think through the design, revise the code, and run the relevant checks.

For a tiny change, the gap can be short, but it should still look like a real edit-review-test cycle rather than an immediate commit.

For a medium change, leave enough time to complete one coherent slice, reconsider the approach, adjust the implementation, and update tests or docs when relevant.

For a large change, leave a longer gap and commit only after a module, migration, adapter capability, storage change, or TUI screen has had time to reach a reviewable state.

Do not make several commits within a few minutes when the code would realistically take longer to produce thoughtfully. Do not wait so long that unrelated changes pile up in one commit.

## TokenTrace-Specific Workflow Examples
Use issue examples tied to the MVP:
- Create initial Rust workspace and CLI shell
- Implement TokenTrace-owned session model
- Import Claude Code OpenTelemetry fixture data
- Add command-based git summary provider
- Render TUI overview and warning screens

Use branch examples like:
```bash
git switch -c cli-foundation
git switch -c claude-adapter
git switch -c git-correlation
```



