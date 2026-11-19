#!/bin/sh
# Mock of install.sh's first screen: prints only, installs nothing.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B=$(printf '\033[1m'); D=$(printf '\033[2m'); R=$(printf '\033[0m')
  V=$(printf '\033[1;36m'); N=$(printf '\033[33m'); G=$(printf '\033[32m')
  Z=$(printf '\033[1;38;5;201m')
else
  B=; D=; R=; V=; N=; G=; Z=
fi

# zaf in its own colour, then back to the colour around it ($1).
zc() { command sed "s/zaf/${Z}zaf${R}$1/g"; }

printf '\n%szaf%s: %sZ%suri'"'"'s %sA%smazing %sF%sramework\n' "$Z" "$R" "$Z" "$R" "$Z" "$R" "$Z" "$R"
printf 'It runs a software project'"'"'s workflow %s(plan, build, review, release)%s\n' "$D" "$R"
printf 'by driving your AI agents for you. %sToday that agent is Claude Code.%s\n\n' "$D" "$R"
printf '%sWhere should %szaf%s%s be installed?%s [%s~/.local/bin%s] \n\n' "$B" "$Z" "$R" "$B" "$R" "$N" "$R"
printf 'This script will do %s5 things%s:\n\n' "$B" "$R"
row() {
  noun=$(printf '%-34s' "$3" | zc "$N")
  why=$(printf '%s' "$4" | zc "$D")
  printf '  %s%s.%s %s%-8s%s%s%s%s %s%s%s\n' "$D" "$1" "$R" "$V" "$2" "$R" "$N" "$noun" "$R" "$D" "$why" "$R"
}
row 1 Check   'git, Claude Code 2.1.296+'       'zaf drives both; nothing is installed for you'
row 2 Install 'zaf 0.5.2 in ~/.local/bin'       'the program you will type commands to'
row 3 Add     '~/.local/bin to PATH (~/.zshrc)' 'so zaf works in any terminal'
row 4 Add     'the teams plugin to Claude Code' 'the roles zaf starts inside Claude Code'
row 5 Show    'your first three commands'       'so you know what to type next'
printf '\nIt changes %snothing else%s, and it does not touch any project.\n\n' "$B" "$R"
printf "%sContinue?%s [%sY%s/n] " "$B" "$R" "$G" "$R"; read -r ans; printf "\n"

# What follows is pretend: every step only sleeps, then says done.
case "$ans" in [nN]*) printf 'Nothing changed.\n'; exit 0;; esac
OK=$(printf '\033[32m')
frames='⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏'
# spin <verb> <noun> <done text> <steps> [bar]
spin() {
  i=0
  while [ "$i" -lt "$4" ]; do
    f=$(printf '%s' "$frames" | cut -d' ' -f$((i % 10 + 1)))
    extra=
    if [ -n "${5:-}" ]; then
      fill=$((i * 20 / $4)); bar=
      j=0; while [ "$j" -lt 20 ]; do
        if [ "$j" -lt "$fill" ]; then bar="$bar━"; else bar="$bar${D}━${R}${G}"; fi; j=$((j + 1)); done
      extra="  ${G}${bar}${R} ${D}$((i * 68 / $4 / 10)).$((i * 68 / $4 % 10))/6.8 MB${R}"
    fi
    printf '\r\033[K  %s%s%s %s%-8s%s%s%s%s' "$V" "$f" "$R" "$V" "$1" "$R" "$N" "$(printf '%s' "$2" | zc "$N")" "$R$extra"
    sleep 0.08; i=$((i + 1))
  done
  printf '\r\033[K  %s✓%s %s%-8s%s%s%s%s\n' "$OK" "$R" "$V" "$1" "$R" "$N" "$(printf '%s' "$3" | zc "$N")" "$R"
}
spin Check   'git and Claude Code'             'git 2.47, Claude Code 2.1.301' 12
spin Install 'zaf 0.5.2'                       'zaf 0.5.2 in ~/.local/bin' 40 bar
spin Add     '~/.local/bin to PATH'            '~/.local/bin to PATH in ~/.zshrc' 6
spin Add     'the teams plugin to Claude Code' 'the teams plugin 2.1.4 to Claude Code' 25
printf '\n%szaf%s is installed. %sOpen a new terminal%s first, so PATH picks it up.\n' "$Z" "$R" "$B" "$R"
printf '\nStart working on anything in %s3 commands%s, from your project'"'"'s folder:\n\n' "$B" "$R"
# pad the plain text first, then colour it, so the columns line up
cmd() { printf '  %s%-3s%s%s%s%s %s%s%s\n' "$D" "$1" "$R" "$V" "$(printf '%-34s' "$2" | zc "$V")" "$R" "$D" "$(printf '%s' "$3" | zc "$D")" "$R"; }
cmd 1. 'zaf setup'                         'connects zaf to your project: board, forge, check'
cmd 2. 'zaf begin --version 0.1.0'         'you say what you want; it becomes objectives, a design, tickets'
cmd 3. 'zaf spawn --watch --version 0.1.0' 'zaf'"'"'s agents build the tickets until they are done'
printf '\n'
cmd '' 'zaf hub'                           'watch them work, live'
cmd '' 'zaf status'                        'where everything stands, on one screen'
cmd '' 'zaf help'                          'every command; add --help to any one'
printf '\n'
printf '%sThe teams plugin is what zaf starts inside Claude Code; you never call it yourself.%s\n\n' "$D" "$R"
