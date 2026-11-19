#!/bin/sh
# Mock of `zaf begin --version 0.1.0` for a stranger: prints only, opens no Claude session.
# Where the real one opens Claude Code, the mock waits for Enter instead.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B=$(printf '\033[1m'); D=$(printf '\033[2m'); R=$(printf '\033[0m')
  V=$(printf '\033[1;36m'); N=$(printf '\033[33m'); G=$(printf '\033[32m')
  Z=$(printf '\033[1;38;5;201m'); OK=$(printf '\033[32m')
else
  B=; D=; R=; V=; N=; G=; Z=; OK=
fi
zc() { command sed "s/zaf/${Z}zaf${R}$1/g"; }
cmd() { printf '%s' "$1" | zc "$V" | command sed "s/^/$V/;s/\$/$R/"; }

# stage <mark> <n> <name> <what>; mark is ▸ (here), ✓ (done) or blank (later)
stage() {
  case "$1" in
    ✓) m="${OK}✓${R}" ;; ▸) m="${V}▸${R}" ;; *) m=' ' ;;
  esac
  if [ "$1" = ▸ ]; then name="$B$(printf '%-11s' "$3")$R"; else name="$(printf '%-11s' "$3")"; fi
  printf '  %s %s%s.%s %s %s%s%s\n' "$m" "$D" "$2" "$R" "$name" "$D" "$(printf '%s' "$4" | zc "$D")" "$R"
}
stages() {
  stage "$1" 1 Objectives 'you say what 0.1.0 is for; Claude asks, writes it down, you sign'
  stage "$2" 2 Design     'Claude drafts how to build it; you answer its questions, you sign'
  stage "$3" 3 Epics      'the design becomes epics of tickets; you sign each epic'
  stage "$4" 4 Build      'zaf builds the signed tickets; you watch, and answer when asked'
}
# session <phase> <file>: where zaf hands the terminal to Claude Code
session() {
  printf '\n%szaf%s now opens %sClaude Code%s for the %s.\n' "$Z" "$R" "$B" "$R" "$1"
  printf '%sAnswer in your own words. Leave Claude (%s/exit%s) when the file says what you mean.%s\n\n' "$D" "$V" "$D" "$R"
  printf '%sOpen it?%s [%sY%s/n] ' "$B" "$R" "$G" "$R"
  read -r ans || ans=
  case "$ans" in [nN]*) printf '\nNothing changed. Run %s when you are ready.\n' "$(cmd 'zaf begin --version 0.1.0')"; exit 0;; esac
  printf '\n%s[ Claude Code runs here: the %s. Press Enter to leave it. ]%s ' "$D" "$1" "$R"
  read -r _ || :
  printf '\n\n  %s✓%s %s%s%s written\n' "$OK" "$R" "$N" "$2" "$R"
  printf '%sSign it?%s It says what you mean, and an edit later unsigns it. [%sY%s/n] ' "$B" "$R" "$G" "$R"
  read -r ans || ans=
  case "$ans" in
    ""|[yY]*) printf '  %s✓%s %s%s%s signed\n' "$OK" "$R" "$N" "$2" "$R" ;;
    *) printf '\nNot signed. Run %s to open it again.\n\n' "$(cmd 'zaf begin --version 0.1.0')"; exit 1 ;;
  esac
}

printf '\nRelease %s0.1.0%s goes through %s4 stages%s. You are at the first:\n\n' "$N" "$R" "$B" "$R"
stages ▸ '' '' ''
session 'objectives interview' '.zaf/releases/0.1.0/objectives.md'

printf '\n'
stages ✓ ▸ '' ''
session 'design' '.zaf/releases/0.1.0/design.md'

printf '\n'
stages ✓ ✓ ▸ ''
printf '\nThe design became %s2 epics%s. Read each, then sign it to let %szaf%s build it:\n\n' "$B" "$R" "$Z" "$R"
epic() {
  printf '  %s%-4s%s %-36s %s%s%s\n' "$N" "$1" "$R" "$2" "$D" "$3" "$R"
  printf '       %sread%s %s   %ssign%s %s\n' "$D" "$R" "$(cmd "zaf epic show $1")" "$D" "$R" "$(cmd "zaf epic sign $1")"
}
epic 12 'Accounts: sign up and sign in' '6 tickets'
epic 13 'Profile page'                  '3 tickets'
printf '\nOnce they are signed, start the build:\n\n'
printf '  %s   %s%s%s\n\n' "$(cmd 'zaf spawn --watch --version 0.1.0')" "$D" "$(printf 'leave it running; zaf hub shows it live' | zc "$D")" "$R"
