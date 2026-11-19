#!/usr/bin/env bash
# Mock of `zaf spawn --watch --version 0.1.0` for a stranger: one view, redrawn in place.
# Everything is pretend: no Claude, no pull requests. One question stops for your answer.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B=$'\e[1m'; D=$'\e[2m'; R=$'\e[0m'; V=$'\e[1;36m'; N=$'\e[33m'
  Z=$'\e[1;38;5;201m'; OK=$'\e[32m'; Q=$'\e[1;38;5;141m'
else
  B=; D=; R=; V=; N=; Z=; OK=; Q=
fi
zc() { local s=${1//zaf/${Z}zaf${R}$2}; printf '%s' "$s"; }
frames=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏)

# ticket: id, epic (0 or 1), title, start step, what it waits on
ids=(21 22 23 27 24 28 26 25 29)
declare -A epic title start waits
t() { epic[$1]=$2; title[$1]=$3; start[$1]=$4; waits[$1]=$5; }
t 21 0 'Sign-up form'     0  ''
t 22 0 'Password rules'   1  ''
t 23 0 'Sign-in'          2  ''
t 24 0 'Sessions'         8  '#21'
t 25 0 'Sign-out'         15 '#24'
t 26 0 'Reset password'   10 '#23'
t 27 1 'Profile model'    3  ''
t 28 1 'Profile page'     10 '#27'
t 29 1 'Avatar upload'    16 '#28'
epics=('Accounts: sign up and sign in' 'Profile page')
LEN=7          # steps from start to merged
QSTEP=4        # the step #23 asks its question
answered=

# state of a ticket at step k: waits | implement | ci | review | merged
state() {
  local d=$(( $2 - ${start[$1]} ))
  if   (( d < 0 )); then echo waits
  elif (( d < 3 )); then echo implement
  elif (( d < 5 )); then echo ci
  elif (( d < LEN )); then echo review
  else echo merged; fi
}
what() {
  case $1 in
    implement) echo 'Claude writes the code' ;;
    ci)        echo 'your check command runs on its pull request' ;;
    review)    echo 'Claude reviews the change' ;;
  esac
}

lines=0
draw() {  # draw <step> <spinner frame> [question]
  local k=$1 f=${frames[$2 % 10]} out='' done=0 total=${#ids[@]}
  for id in "${ids[@]}"; do [ "$(state "$id" "$k")" = merged ] && done=$((done + 1)); done
  out+="$(zc 'zaf' "$R") is building ${N}0.1.0${R}  ${V}${f}${R}  ${B}${done} of ${total}${R} tickets merged  ${D}${k} min${R}"$'\n'
  out+="${D}Ctrl-C stops it; work in progress is kept, and the same command picks it up.${R}"$'\n\n'
  local e
  for e in 0 1; do
    local n=0 m=0 bar=''
    for id in "${ids[@]}"; do
      [ "${epic[$id]}" = "$e" ] || continue
      n=$((n + 1)); [ "$(state "$id" "$k")" = merged ] && m=$((m + 1))
    done
    for ((i = 0; i < n; i++)); do if ((i < m)); then bar+="${OK}■${R}"; else bar+="${D}□${R}"; fi; done
    out+="  ${B}$(printf '%-34s' "${epics[$e]}")${R} ${bar} ${D}${m}/${n}${R}"$'\n'
    for id in "${ids[@]}"; do
      [ "${epic[$id]}" = "$e" ] || continue
      local s; s=$(state "$id" "$k")
      local name; name=$(printf '%-16s' "${title[$id]}")
      if [ -n "$3" ] && [ "$id" = 23 ]; then
        out+="    ${N}#${id}${R} ${name} ${Q}? question for you${R}"$'\n'
        continue
      fi
      case $s in
        merged) out+="    ${N}#${id}${R} ${name} ${OK}✓ merged${R}"$'\n' ;;
        waits)  if [ -n "${waits[$id]}" ]; then w="waits on ${waits[$id]}"; else w=queued; fi
                out+="    ${D}#${id} ${name} · ${w}${R}"$'\n' ;;
        *)      out+="    ${N}#${id}${R} ${name} ${V}${f} $(printf '%-9s' "$s")${R} ${D}$(what "$s")${R}"$'\n' ;;
      esac
    done
  done
  # move up over the last frame, clear it, print this one
  (( lines > 0 )) && printf '\e[%dA\e[J' "$lines"
  printf '%s' "$out"
  lines=$(printf '%s' "$out" | wc -l)
}

printf '\n'; lines=0
last=$(( ${start[29]} + LEN ))
for ((k = 0; k <= last; k++)); do
  if (( k == QSTEP )) && [ -z "$answered" ]; then
    draw "$k" 0 question
    printf '\n  %s#23 asks:%s Should sign-in also accept a username, or the email only?\n' "$Q" "$R"
    printf '  %sThe rest keeps building while you think. Answer here, or from any terminal:%s\n' "$D" "$R"
    printf '  %s %s'"'"'your answer'"'"'%s\n\n' "$(zc 'zaf' "$V")" "${V}board ruling 23 ${R}${D}" "$R"
    printf '  %sYour answer:%s ' "$B" "$R"
    read -r answered || answered=email
    [ -n "$answered" ] || answered='email only'
    printf '  %s✓%s recorded; #23 goes on.\n\n' "$OK" "$R"
    sleep 1
    lines=0
  fi
  for ((s = 0; s < 4; s++)); do draw "$k" "$((k * 4 + s))"; sleep 0.1; done
done

printf '\n%s✓%s %s built %s0.1.0%s: %s9 tickets%s merged in 23 min.\n\n' "$OK" "$R" "$(zc 'zaf' "$R")" "$N" "$R" "$B" "$R"
printf '%sNext:%s review the release pull request %s#30%s and merge it to ship 0.1.0.\n' "$B" "$R" "$N" "$R"
printf '%sThen start the next release:%s %s%s\n\n' "$D" "$R" "$(zc 'zaf' "$V")" "${V} begin --version 0.2.0${R}"
