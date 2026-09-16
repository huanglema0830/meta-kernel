#!/usr/bin/env bash
# 云内核项目 · 把 origin 切到「SSH over 443」（D19 执行入口）
#
# 为什么要切：HTTPS + Windows Schannel 路径上存在间歇性连接超时（≈5%，与 TLS 重协商关联），
# 导致推送要反复重试（实测最坏一次 11 轮 / 14 分钟）。SSH over 443 可绕开该路径。
# 详见：coordination/reports/2026-09-16_R2推送连接超时根因报告.md
#
# 前提：本机公钥 ~/.ssh/id_ed25519_metakernel.pub 已添加到 GitHub 账号
#       （Settings → SSH and GPG keys → New SSH key）
#
# ⚠️ 实测教训（2026-09-16）：本机 Git Bash 的 ssh **不会加载 `~/.ssh/config`**
#    （`ssh -v` 只打印 "Reading configuration data /etc/ssh/ssh_config"，用户级 config 被跳过；
#     仅当 `ssh -F ~/.ssh/config` 显式指定时才生效——`ssh -G` 亦同）。
#    因此本脚本**不依赖 config 别名**，改用 `core.sshCommand` 把「key 路径 + 443 端口」显式交给 git。
#    （`core.sshCommand` 存于 `.git/config`，属**本地**配置——不入库、不外发。）
#
# 幂等 + 可回退：已是 SSH 则仅校正 sshCommand；验证失败自动回退 HTTPS。
set -uo pipefail

REPO="huanglema0830/meta-kernel"
SSH_URL="ssh://git@ssh.github.com:443/${REPO}.git"
HTTPS_URL="https://github.com/${REPO}.git"
KEY="${HOME}/.ssh/id_ed25519_metakernel"

cd "$(dirname "$0")/.." || exit 1
echo "仓库：$(pwd)"

# Windows 形式路径——原生 ssh.exe 需要它（POSIX 形式 /c/... 无法被 Windows API 解析）
KEY_WIN="$(cygpath -m "$KEY" 2>/dev/null || echo "$KEY")"

echo
echo "===== 1) 确认私钥存在 ====="
if [ -f "$KEY" ]; then
  echo "  ✅ 私钥就位，指纹："
  ssh-keygen -lf "${KEY}.pub" 2>/dev/null | sed 's/^/     /'
else
  echo "  ❌ 找不到私钥：$KEY"
  echo "     请先运行（生成后需你在 GitHub 账号添加公钥）："
  echo "     ssh-keygen -t ed25519 -C \"meta-kernel-workbuddy\" -f \"$KEY\" -N \"\""
  exit 1
fi

echo
echo "===== 2) 检测公钥是否已在 GitHub 生效（显式 key + 443，不依赖 config）====="
# ⚠️ 本机 ssh 在尝试写 known_hosts 时会因**用户名编码问题**失败并返回 1
#    （报 'Could not create directory /c/Users/<乱码>/.ssh'），**即使认证已成功**。
#    因此这里「捕获输出后判断文本」，**不看 ssh 的退出码**。
AUTH_OUT="$(timeout 40 ssh -i "$KEY_WIN" -p 443 -o IdentitiesOnly=yes -T git@ssh.github.com 2>&1 || true)"
if printf '%s' "$AUTH_OUT" | grep -q "successfully authenticated"; then
  echo "  ✅ 公钥已生效"
else
  echo "  ⏳ 公钥尚未生效——请先在 GitHub 账号添加："
  echo "     Settings → SSH and GPG keys → New SSH key → 粘贴 ${KEY}.pub 的内容"
  exit 1
fi

echo
echo "===== 3) 切换 origin + 显式 sshCommand ====="
CUR="$(git remote get-url origin)"
echo "  当前：$CUR"
if [ "$CUR" = "$SSH_URL" ]; then
  echo "  remote 已是 SSH over 443"
else
  git remote set-url origin "$SSH_URL"
  echo "  remote → $SSH_URL"
fi
git config core.sshCommand "ssh -i \"$KEY_WIN\" -p 443 -o IdentitiesOnly=yes"
echo "  core.sshCommand 已设置（显式 key + 443；本地配置，不入库）"
echo "  ⚠️ 刻意**不加** StrictHostKeyChecking=accept-new：本机写 known_hosts 会因用户名编码问题失败并返回 1，"
echo "     反而干扰判断（host key 已在 known_hosts 中，无需再写）。"

echo
echo "===== 4) 验证通道（ls-remote）====="
if timeout 90 git ls-remote origin -h refs/heads/main >/dev/null 2>&1; then
  echo "  ✅ SSH over 443 通道可用（拉取/推送正常）"
else
  echo "  ❌ 验证失败 → 自动回退 HTTPS"
  git remote set-url origin "$HTTPS_URL"
  git config --unset core.sshCommand 2>/dev/null || true
  exit 1
fi

echo
echo "===== 5) 完成 ====="
git remote -v
echo
echo "回退（随时可执行）："
echo "  git remote set-url origin $HTTPS_URL && git config --unset core.sshCommand"
echo "彻底回退：在 GitHub 账号删除该公钥 + 删本机 ~/.ssh/id_ed25519_metakernel*"
