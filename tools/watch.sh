#!/usr/bin/env bash
# Watches for outside work on the VIctory fork and upstream VIctory, using no tokens while nothing changes.
# It waits $INTERVAL seconds, then checks:
#   - the newest commit on fxceless/victory-fork main and on Marvisak/VIctory main (git ls-remote)
#   - pull requests and issues numbered above the last seen ones (GitHub search API, via gh)
# When anything is new, it appends a block to $STATE/inbox.txt, advances its cursor and exits 0, so the session
# that started it in the background is woken to triage. It exits 2 on repeated errors.
#
# usage: watch.sh STATE_DIR [INTERVAL_SECONDS]
# First run: seed STATE_DIR with files fork_main, up_main, fork_num, up_num (see PROGRESS.md "Read cursor").
set -u
STATE=${1:?state dir}
INTERVAL=${2:-600}
mkdir -p "$STATE"
FORK=fxceless/victory-fork
UP=Marvisak/VIctory
errors=0

read_state() { cat "$STATE/$1" 2>/dev/null || echo "${2:-}"; }

newer() { # repo, last number, kind (pr|issue) -> lines "number<TAB>title<TAB>author"
    gh api -X GET search/issues -f q="repo:$1 is:$3 created:>=$(date -u -d '-3 days' +%Y-%m-%d)" \
        -f sort=created -f order=desc -f per_page=100 \
        --jq ".items[] | select(.number > $2) | \"\(.number)\t\(.title)\t\(.user.login)\"" 2>/dev/null
}

while :; do
    # Wait first, so that a restart right after a wake gathers a whole interval's worth of items.
    sleep "$INTERVAL"
    found=""
    fork_main_now=$(git ls-remote "https://github.com/$FORK" refs/heads/main 2>/dev/null | cut -c1-40)
    up_main_now=$(git ls-remote "https://github.com/$UP" refs/heads/main 2>/dev/null | cut -c1-40)
    fork_num=$(read_state fork_num 0)
    up_num=$(read_state up_num 0)
    fork_items=$( { newer $FORK "$fork_num" pr; newer $FORK "$fork_num" issue; } | sort -n -u)
    up_items=$(newer $UP "$up_num" pr | sort -n -u)

    if [ -z "$fork_main_now" ] || [ -z "$up_main_now" ]; then
        errors=$((errors + 1))
        [ $errors -ge 5 ] && { echo "watch: 5 failed checks in a row" >&2; exit 2; }
        continue
    fi
    errors=0

    fork_main=$(read_state fork_main)
    up_main=$(read_state up_main)
    {
        echo "=== $(date -u +%Y-%m-%dT%H:%MZ)"
        if [ "$fork_main_now" != "$fork_main" ]; then echo "fork main: ${fork_main:0:7} -> ${fork_main_now:0:7}"; fi
        if [ "$up_main_now" != "$up_main" ]; then echo "upstream main: ${up_main:0:7} -> ${up_main_now:0:7}"; fi
        [ -n "$fork_items" ] && { echo "fork PRs/issues:"; echo "$fork_items" | sed 's/^/  #/'; }
        [ -n "$up_items" ] && { echo "upstream PRs:"; echo "$up_items" | sed 's/^/  #/'; }
    } > "$STATE/pending.txt"

    if [ "$fork_main_now" != "$fork_main" ] || [ "$up_main_now" != "$up_main" ] || [ -n "$fork_items" ] || [ -n "$up_items" ]; then
        found=yes
    fi
    if [ -n "$found" ]; then
        cat "$STATE/pending.txt" >> "$STATE/inbox.txt"
        echo "$fork_main_now" > "$STATE/fork_main"
        echo "$up_main_now" > "$STATE/up_main"
        [ -n "$fork_items" ] && echo "$fork_items" | cut -f1 | sort -n | tail -1 > "$STATE/fork_num"
        [ -n "$up_items" ] && echo "$up_items" | cut -f1 | sort -n | tail -1 > "$STATE/up_num"
        cat "$STATE/pending.txt"
        exit 0
    fi
done
