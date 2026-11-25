#!/usr/bin/env bash
# Lists, for each repository in tools/fork-watch.txt, the commits it has
# that none of the reference branches has (upstream, fxceless, ours):
# what that project carries of its own. Only new commits since the last
# run are marked NEW (state kept in .git/fork-watch/, outside the tree).
#
#   tools/fork-watch.sh [reference-ref...]   (default: upstream/main fxceless/main HEAD)
#
# Read-only for the working tree: it adds remotes named watch-<owner>-<repo>
# and fetches them. Fetched code is only read, never built or run here.
set -u
cd "$(git rev-parse --show-toplevel)"
list=tools/fork-watch.txt
state="$(git rev-parse --git-common-dir)/fork-watch"
mkdir -p "$state"
refs=("$@"); [ ${#refs[@]} -eq 0 ] && refs=(upstream/main fxceless/main HEAD)
exclude=(); for r in "${refs[@]}"; do exclude+=("^$r"); done
grep -v '^\s*#' "$list" | while read -r slug note; do
  [ -z "$slug" ] && continue
  remote="watch-${slug//\//-}"
  git remote get-url "$remote" >/dev/null 2>&1 || git remote add "$remote" "https://github.com/$slug"
  if ! GIT_LFS_SKIP_SMUDGE=1 timeout 300 git fetch -q --no-tags "$remote" 2>/dev/null; then
    echo "== $slug: fetch failed"; continue
  fi
  echo "== $slug ${note:-}"
  for b in $(git for-each-ref --format='%(refname:short)' "refs/remotes/$remote/"); do
    n=$(git rev-list --count --no-merges "$b" "${exclude[@]}")
    [ "$n" -eq 0 ] && continue
    seen="$state/${b//\//_}"
    echo "  $b: $n commit(s) of its own (head $(git rev-parse --short "$b"), base upstream $(git merge-base "$b" upstream/main | cut -c1-8))"
    git log --no-merges --format='%H %h %an %ad %s' --date=format:'%m-%d %H:%M' "$b" "${exclude[@]}" | head -40 |
      while read -r full short rest; do
        if [ -f "$seen" ] && grep -q "$full" "$seen"; then mark="    "; else mark="NEW "; fi
        echo "    $mark$short $rest" | cut -c1-160
      done
    git rev-list --no-merges "$b" "${exclude[@]}" > "$seen"
  done
done
