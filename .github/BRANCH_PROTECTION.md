# main 分支保护（期望配置）

目标：**拥有者可直接维护 main，其他协作者必须走 Pull Request，不能直接 push / force push / 删除。**

期望的 GitHub 规则：

- Require a pull request before merging（至少 1 个 approval）
- Require review from Code Owners（`.github/CODEOWNERS` = `@CuKhros`）
- Dismiss stale pull request approvals when new commits are pushed
- Require conversation resolution before merging
- Do not allow force pushes
- Do not allow deletions
- `enforce_admins = false`：拥有者保留应急直推能力；非拥有者没有 push 权限，也无法绕过 PR

当前状态（2026-11-23）：仓库为 **private + GitHub Free**，`branches/main/protection` 与 `rulesets` API 均返回 403（GitHub 要求 Pro 或 public）。现有唯一协作者 `yuyuyuyumin` 只有 `pull` 权限，**实际上不能写 main**；但“可审计的分支保护规则”要等升级 Pro 或仓库公开后才能创建。此文档不表示规则已经生效。

升级 Pro 后可执行（拥有者）：

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

注意：仓库公开前不要为了开启保护而切换可见性；公开是独立决策，必须由拥有者明确执行。
