---
name: land
description: >-
  Land this project's changes through a GitHub PR only when explicitly requested.
  Do not use for review, preparation, or skill installation.
metadata:
  delta-action: land
---

# Land changes

1. Confirm authorization and scope. Inspect the working tree and remotes.
   Preserve unrelated edits. Publish to the GitHub remote, never the `local` remote.
2. Commit the requested changes on a topic branch. Fetch the latest remote `main`.
   Rebase onto it before opening the PR. Resolve only mechanical conflicts automatically.
   Stop for conflicts that require a design or behavior decision. Ask in the thread before further changes.
3. Run the checks in `.github/workflows/ci.yml` locally when their dependencies are available.
   Push the topic branch and open a PR targeting `main`. Do not push directly to `main`.
4. Wait for CI and review-bot approval on the current PR commit.
   Inspect checks, reviews, review requests, issue comments, inline comments, and reactions.
   Wait for every review bot that announces work to finish, even after CI passes.
   If no review bot participates, ask before treating review as satisfied.
5. Adopt trivial feedback that preserves behavior, then rerun checks and wait for updated reviews.
   Stop for feedback that requires a design or behavior decision. Ask in the thread before further changes.
   Ask before force-pushing. When authorized, use `--force-with-lease`.
6. Fetch `main` again and confirm that it is an ancestor of the approved PR head.
   If it advanced, rebase and repeat the checks and reviews before merging.
   Use GitHub's rebase merge with `--match-head-commit` set to the approved head. Never create a merge commit.
7. Fetch and verify the change on remote `main`. Report the PR, landed commit, and CI result.
   In Delta, load `report-landed` and follow its result format.
