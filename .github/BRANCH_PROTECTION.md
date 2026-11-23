# main branch protection

Goal: **the owner can maintain main directly; every other collaborator must go through a Pull Request and cannot push, force-push, or delete main.**

**Status: active.** The rule was applied through the GitHub API on 2026-11-23 after the repository became public. The owner keeps the `enforce_admins = false` emergency direct-push path; non-owners still need a Pull Request.

Effective rule:

- Require a pull request before merging (at least 1 approval)
- Require review from Code Owners (`.github/CODEOWNERS` = `@CuKhros`)
- Dismiss stale pull request approvals when new commits are pushed
- Require conversation resolution before merging
- Do not allow force pushes
- Do not allow deletions
- `enforce_admins = false`: the owner keeps an emergency direct-push path; non-owners have no push permission and cannot bypass the PR flow

Current state (2026-11-23): the repository is **public** and the protection rule above is active. `yuyuyuyumin` has `pull` only; any future write access still goes through a Pull Request.

The same payload can be re-applied with:

```bash
cat <<'JSON' | gh api --method PUT repos/CuKhros/FeatherAgent/branches/main/protection --input -
{
  "required_status_checks": null,
  "enforce_admins": false,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": true,
    "required_approving_review_count": 1
  },
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_conversation_resolution": true
}
JSON
```

Visibility is public by explicit owner decision (2026-11-23); branch protection is a separate control and should not be used as a reason to change visibility silently.
