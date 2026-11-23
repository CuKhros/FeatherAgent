# main branch protection (intended configuration)

Goal: **the owner can maintain main directly; every other collaborator must go through a Pull Request and cannot push, force-push, or delete main.**

Intended GitHub rule:

- Require a pull request before merging (at least 1 approval)
- Require review from Code Owners (`.github/CODEOWNERS` = `@CuKhros`)
- Dismiss stale pull request approvals when new commits are pushed
- Require conversation resolution before merging
- Do not allow force pushes
- Do not allow deletions
- `enforce_admins = false`: the owner keeps an emergency direct-push path; non-owners have no push permission and cannot bypass the PR flow

Current state (2026-11-23): the repository is **private + GitHub Free**; both `branches/main/protection` and `rulesets` APIs return 403 (GitHub requires Pro or public). The only collaborator, `yuyuyuyumin`, has `pull` only and **cannot write main in practice**; an auditable protection rule still needs Pro or a public repository. This document does not claim the rule is active.

After upgrading to Pro, the owner can run:

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

Note: do not change visibility merely to enable protection; going public is an independent decision that the owner must make explicitly.
