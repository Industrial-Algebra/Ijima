# Contributing Guidelines

## Git Flow

- **Feature branches**: `feature/<name>`
- **Bugfix / chore branches**: `bugfix/<name>` or `chore/<name>`
- **Develop**: Integration branch where PRs are merged.
- **Release**: When ready, create a PR from `develop` to `main` for a new release.

1. Create a branch from `develop`.
2. Work on your changes.
3. Open a PR targeting `develop`.
4. After CI passes and PR is approved, merge into `develop`.
5. When a release is needed, open a PR from `develop` to `main`.

## Pre-commit Hook

Copy the pre-commit script to your local git hooks:

```bash
mkdir -p .git/hooks
cp .githooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

This will run `cargo fmt`, `cargo clippy`, and `cargo test` on every commit.
