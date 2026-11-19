#!/usr/bin/env bash
# Mock of `zaf update` for a stranger: prints only, replaces nothing.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B=$'\e[1m'; D=$'\e[2m'; R=$'\e[0m'; V=$'\e[1;36m'; N=$'\e[33m'; G=$'\e[32m'
  Z=$'\e[1;38;5;201m'; OK=$'\e[32m'; ADD=$'\e[32m'; DEL=$'\e[31m'
else
  B=; D=; R=; V=; N=; G=; Z=; OK=; ADD=; DEL=
fi
zc() { local s=${1//zaf/${Z}zaf${R}$2}; printf '%s' "$s"; }
frames=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏)
# pad by characters, not bytes, so an arrow does not shift the column
pad() { local s=$1; printf '%s%*s' "$s" $(( $2 - ${#s} )) ''; }
row() { printf '  %s%s.%s %s%-8s%s%s%s%s %s%s%s\n' "$D" "$1" "$R" "$V" "$2" "$R" "$N" "$(zc "$(pad "$3" 30)" "$N")" "$R" "$D" "$(zc "$4" "$D")" "$R"; }
spin() {  # spin <verb> <noun> <done noun> <ticks>
  local i
  for ((i = 0; i < $4; i++)); do
    printf '\r\e[K  %s%s%s %s%-8s%s%s%s%s' "$V" "${frames[i % 10]}" "$R" "$V" "$1" "$R" "$N" "$(zc "$2" "$N")" "$R"
    sleep 0.08
  done
  printf '\r\e[K  %s✓%s %s%-8s%s%s%s%s\n' "$OK" "$R" "$V" "$1" "$R" "$N" "$(zc "$3" "$N")" "$R"
}

printf '\n%s Newer versions are out. This will do %s3 things%s:\n\n' "$(zc 'zaf update:' "$R")" "$B" "$R"
row 1 Replace 'zaf 0.5.1 → 0.5.2'            'the program itself, in ~/.local/bin'
row 2 Update  'teams plugin 2.1.4 → 2.1.5'   'the roles zaf starts inside Claude Code'
row 3 Update  '2 of 12 actions in ~/.zaf'    'the steps zaf runs; each change is shown first'

printf '\n%sWhat is new%s\n' "$B" "$R"
printf '  %s•%s A question for you shows in %s, and you can answer it there.\n' "$V" "$R" "$(zc 'zaf spawn --watch' "$V")$R"
printf '  %s•%s %s lists only what failed, with the command that fixes it.\n' "$V" "$R" "$(zc 'zaf setup' "$V")$R"
printf '  %s•%s The %smerge%s action waits for the checks before it merges.\n' "$V" "$R" "$N" "$R"
printf '  %sAll of it: https://github.com/ProgrammingCheetah/ways/releases/tag/zaf-v0.5.2%s\n\n' "$D" "$R"

printf '%sContinue?%s [%sY%s/n] ' "$B" "$R" "$G" "$R"
read -r ans || ans=
case "$ans" in [nN]*) printf '\nNothing changed.\n\n'; exit 0;; esac
printf '\n'

spin Replace 'zaf 0.5.1 → 0.5.2'          'zaf 0.5.2'              25
spin Update  'teams plugin 2.1.4 → 2.1.5' 'teams plugin 2.1.5'     15

# one action that changes: show why, the difference, then ask
printf '\n  %s%s%s %s%s%s\n' "$V" "merge" "$R" "$D" "will be replaced. Why: it now waits for the checks before it merges." "$R"
printf '  %s~/.zaf/actions/merge%s\n' "$D" "$R"
printf '  %s@@ -12,4 +12,6 @@%s\n' "$D" "$R"
printf '   pr=$1\n'
printf '  %s-gh pr merge "$pr" --squash%s\n' "$DEL" "$R"
printf '  %s+gh pr checks "$pr" --watch --fail-fast || exit 1%s\n' "$ADD" "$R"
printf '  %s+gh pr merge "$pr" --squash%s\n' "$ADD" "$R"
printf '   echo "merged $pr"\n'
printf '  %sReplace it?%s The old one is kept in ~/.zaf/actions-replaced. [%sY%s/n] ' "$B" "$R" "$G" "$R"
read -r ans || ans=
case "$ans" in [nN]*) kept1=1; printf '  %skept yours%s\n' "$D" "$R";; *) printf '  %s✓%s merge replaced\n' "$OK" "$R";; esac

printf '\n  %s%s%s %s%s%s\n' "$V" "ci-wait" "$R" "$D" "will be replaced. Why: a pull request with no check is raised within minutes, not two hours." "$R"
printf '  %s~/.zaf/actions/ci-wait%s\n' "$D" "$R"
printf '  %s@@ -3,1 +3,1 @@%s\n' "$D" "$R"
printf '  %s-timeout=7200%s\n' "$DEL" "$R"
printf '  %s+no_check_after=300%s\n' "$ADD" "$R"
printf '  %sReplace it?%s The old one is kept in ~/.zaf/actions-replaced. [%sY%s/n] ' "$B" "$R" "$G" "$R"
read -r ans || ans=
case "$ans" in [nN]*) kept2=1; printf '  %skept yours%s\n' "$D" "$R";; *) printf '  %s✓%s ci-wait replaced\n' "$OK" "$R";; esac

kept=$(( ${kept1:-0} + ${kept2:-0} ))
printf '\n%s✓%s %s is up to date.' "$OK" "$R" "$(zc 'zaf' "$R")"
if (( kept > 0 )); then
  printf ' %s%d action(s) kept as yours; %s%s shows them again.%s' "$D" "$kept" "$(zc 'zaf update' "$V")" "$D" "$R"
fi
printf '\n%sRunning instances finish on the old version; new ones start on 0.5.2.%s\n\n' "$D" "$R"
