#!/usr/bin/env bash
# Deploy the combined site (website + docs) to the Codeberg Pages branch.
#
# Usage:
#   nix run .#deploy-pages
#   # or directly:
#   bash scripts/deploy-pages.sh

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
REMOTE="${DEPLOY_REMOTE:-origin}"
BRANCH="pages"
COMMIT_MSG="Deploy site $(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo ":: Building site..."
SITE_PATH=$(nix build "${REPO_ROOT}#site" --no-link --print-out-paths)
echo "   Built: ${SITE_PATH}"

# Use a temporary directory for the pages worktree
WORK_DIR=$(mktemp -d)
trap 'rm -rf "${WORK_DIR}"' EXIT

echo ":: Preparing ${BRANCH} branch..."

# Check if the pages branch exists on the remote
if git ls-remote --exit-code "${REMOTE}" "refs/heads/${BRANCH}" >/dev/null 2>&1; then
  # Clone just the pages branch (shallow, single-branch)
  git clone --depth 1 --branch "${BRANCH}" --single-branch \
    "$(git remote get-url "${REMOTE}")" "${WORK_DIR}" --quiet
else
  # Create a fresh orphan branch
  git init "${WORK_DIR}" --quiet
  git -C "${WORK_DIR}" checkout --orphan "${BRANCH}"
  git -C "${WORK_DIR}" remote add "${REMOTE}" "$(git remote get-url "${REMOTE}")"
fi

echo ":: Copying site output..."
# Clear existing content (except .git)
find "${WORK_DIR}" -mindepth 1 -maxdepth 1 ! -name '.git' -exec rm -rf {} +

# Copy built site into the worktree (dereference nix store symlinks) without
# preserving read-only Nix store modes, so the cleanup trap can remove it.
cp -rL --no-preserve=mode "${SITE_PATH}/." "${WORK_DIR}/"

# Codeberg Pages requires a .nojekyll-equivalent or just serves static files directly
# No special file needed for Codeberg, but ensure there's no .gitignore blocking things

echo ":: Committing and pushing..."
cd "${WORK_DIR}"
git add --all
if git diff --cached --quiet; then
  echo "   No changes to deploy."
  exit 0
fi

git commit -m "${COMMIT_MSG}" --quiet
git push "${REMOTE}" "HEAD:${BRANCH}" --force --quiet

echo ":: Deployed to https://modde.tartanoglu.com/"
echo "   Docs at  https://modde.tartanoglu.com/docs/"
