#!/bin/sh
# Mock of `zaf setup` for a stranger: prints only, writes nothing in the project.
# The first run ends with one thing to fix; the next run is green; then it starts over.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B=$(printf '\033[1m'); D=$(printf '\033[2m'); R=$(printf '\033[0m')
  V=$(printf '\033[1;36m'); N=$(printf '\033[33m'); G=$(printf '\033[32m')
  Z=$(printf '\033[1;38;5;201m'); OK=$(printf '\033[32m'); BAD=$(printf '\033[31m')
else
  B=; D=; R=; V=; N=; G=; Z=; OK=; BAD=
fi
marker="${TMPDIR:-/tmp}/.zaf-setup-mock-fixed"

# zaf in its own colour, then back to the colour around it ($1).
zc() { command sed "s/zaf/${Z}zaf${R}$1/g"; }
# a plan row: number, verb, noun, why (padded plain, then coloured)
row() {
  printf '  %s%s.%s %s%-8s%s%s%s%s %s%s%s\n' "$D" "$1" "$R" "$V" "$2" "$R" \
    "$N" "$(printf '%-24s' "$3" | zc "$N")" "$R" "$D" "$(printf '%s' "$4" | zc "$D")" "$R"
}
# a question: label, what it means, default; the answer lands in $answer
ask() {
  printf '  %s%-14s%s %s%s%s\n' "$B" "$1" "$R" "$D" "$(printf '%s' "$2" | zc "$D")" "$R"
  if [ -n "$3" ]; then printf '  %s›%s [%s%s%s] ' "$V" "$R" "$N" "$3" "$R"; else printf '  %s›%s ' "$V" "$R"; fi
  read -r answer || answer=
  [ -n "$answer" ] || answer=$3
}
frames='⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏'
# spin <verb> <noun while running> <noun when done> <ticks> [fail]
spin() {
  i=0
  while [ "$i" -lt "$4" ]; do
    f=$(printf '%s' "$frames" | cut -d' ' -f$((i % 10 + 1)))
    printf '\r\033[K  %s%s%s %s%-8s%s%s%s%s' "$V" "$f" "$R" "$V" "$1" "$R" "$N" "$(printf '%s' "$2" | zc "$N")" "$R"
    sleep 0.08; i=$((i + 1))
  done
  if [ -n "${5:-}" ]; then mark="${BAD}✗"; else mark="${OK}✓"; fi
  printf '\r\033[K  %s%s %s%-8s%s%s%s%s\n' "$mark" "$R" "$V" "$1" "$R" "$N" "$(printf '%s' "$3" | zc "$N")" "$R"
}

printf '\n%s connects %szaf%s to this project. It will do %s5 things%s:\n\n' "$(printf 'zaf setup' | zc "$V" | command sed "s/^/$V/")$R" "$Z" "$R" "$B" "$R"
row 1 Ask     '4 questions'           'what zaf needs to know about this project'
row 2 Write   '.zaf.toml'             'your answers, so zaf remembers them'
row 3 Make    '~/.zaf and .zaf/'      'where zaf keeps its state'
row 4 Install 'the standard actions'  'the steps zaf runs: open a PR, merge, release'
row 5 Check   'your tools and access' 'so the first real run does not fail halfway'
printf '\nIt changes %snothing else%s in the project.\n\n' "$B" "$R"
printf '%sContinue?%s [%sY%s/n] ' "$B" "$R" "$G" "$R"
read -r ans || ans=
case "$ans" in [nN]*) printf '\nNothing changed.\n'; exit 0;; esac
printf '\n'

folder=my-app; repo=zuri/my-app
ask 'Project name'  'what zaf calls this project'                                  "$folder"
ask 'Board'         'where your tickets live: GitHub issues of owner/name'          "$repo"
ask 'Forge'         'where pull requests open: owner/name'                          "$repo"
ask 'Check command' 'the command that proves the code is good, e.g. cargo test (empty: none)' ''
printf '\n'

spin Ask     '4 questions'          '4 questions'          1
spin Write   '.zaf.toml'            '.zaf.toml'            6
spin Make    '~/.zaf and .zaf/'     '~/.zaf and .zaf/'     6
spin Install 'the standard actions' '12 standard actions'  20
if [ -f "$marker" ]; then
  spin Check 'your tools and access' 'your tools and access' 18
  rm -f "$marker"
  printf '\n%szaf%s is set up here. %sNext:%s\n\n' "$Z" "$R" "$B" "$R"
  printf '  %s%s%s %s%s%s\n\n' "$V" "$(printf '%-28s' 'zaf begin --version 0.1.0' | zc "$V")" "$R" \
    "$D" 'you say what you want; it becomes objectives, a design, tickets' "$R"
else
  spin Check 'your tools and access' '1 thing to fix' 18 fail
  : > "$marker"
  printf '\n%sFix it%s, then run %s again:\n\n' "$B" "$R" "$(printf 'zaf setup' | zc "$V" | command sed "s/^/$V/")$R"
  printf '  %s%-20s%s %s%-16s%s %s%s%s\n\n' "$BAD" 'gh is not signed in' "$R" "$V" 'gh auth login' "$R" \
    "$D" "$(printf 'zaf opens pull requests through gh' | zc "$D")" "$R"
fi
