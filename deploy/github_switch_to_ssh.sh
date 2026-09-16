#!/usr/bin/env bash
# 云内核项目 · 把 origin 从 HTTPS 切到「SSH over 443」（D19 的执行入口）
#
# 为什么要切：HTTPS + Windows Schannel 路径上存在间歇性连接超时（≈5%，与 TLS 重协商关联），
# 导致推送要反复重试（实测最坏一次 11 轮 / 14 分钟）。SSH over 443 可绕开该路径。
# 详见：coordination/reports/2026-09-16_R2推送连接超时根因报告.md
#
# 前提：本机公钥 ~/.ssh/id_ed25519_metakernel.pub 已添加到 GitHub 账号
#       （Settings → SSH and GPG keys → New SSH key）
#
# 幂等 + 可回退：已是 SSH 则不动；验证失败会自动回退 HTTPS。
set -uo pipefail

REPO="huanglema0830/meta-kernel"
SSH_URL="github-443:${REPO}.git"
HTTPS_URL="https://github.com/${REPO}.git"

cd "$(dirname "$0")/.." || exit 1
echo "仓库：$(pwd)"

echo
echo "===== 1) 检测公钥是否已在 GitHub 生效 ====="
if timeout 30 ssh -T git@github-443 2>&1 | grep -q "successfully authenticated"; then
  echo "  ✅ 公钥已生效"
else
  echo "  ⏳ 公钥尚未生效——请先在 GitHub 账号添加："
  echo "     Settings → SSH and GPG keys → New SSH key"
  echo "     粘贴内容见：~/.ssh/id_ed25519_metakernel.pub"
  exit 1
fi

echo
echo "===== 2) 切换 origin ====="
CUR="$(git remote get-url origin)"
echo "  当前：$CUR"
if [ "$CUR" = "$SSH_URL" ]; then
  echo "  已是 SSH over 443，无需切换"
else
  git remote set-url origin "$SSH_URL"
  echo "  已切换 → $SSH_URL"
fi

echo
echo "===== 3) 验证通道（ls-remote）====="
if timeout 60 git ls-remote origin -h refs/heads/main >/dev/null 2>&1; then
  echo "  ✅ SSH 通道可用（拉取/推送正常）"
else
  echo "  ❌ SSH 通道验证失败 → 自动回退 HTTPS"
  git remote set-url origin "$HTTPS_URL"
  exit 1
fi

echo
echo "===== 4) 完成 ====="
git remote -v
echo
echo "回退（随时可执行）： git remote set-url origin $HTTPS_URL"
echo "彻底回退：在 GitHub 账号删除该公钥 + 删本机 ~/.ssh/id_ed25519_metakernel*"
