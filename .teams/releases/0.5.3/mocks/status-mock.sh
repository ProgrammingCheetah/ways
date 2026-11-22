#!/usr/bin/env bash
# Mock of `zaf status` in 0.5.3: what waits on you first, colour where the eye should go.
# The data is ways' board on 2026-11-19, as `zaf status --more` printed it. Prints only.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B=$'\e[1m'; D=$'\e[2m'; R=$'\e[0m'; V=$'\e[1;36m'; N=$'\e[33m'
  Z=$'\e[1;38;5;201m'; OK=$'\e[32m'; BAD=$'\e[1;31m'; Q=$'\e[1;38;5;141m'; QL=$'\e[38;5;141m'
else
  B=; D=; R=; V=; N=; Z=; OK=; BAD=; Q=; QL=
fi
pad() { local s=$1; printf '%s%*s' "$s" $(( $2 - ${#s} )) ''; }
bar() {  # bar <done> <total>: one square per ticket, done in green
  local i out=''
  for ((i = 0; i < $2; i++)); do if ((i < $1)); then out+="${OK}■${R}"; else out+="${D}□${R}"; fi; done
  printf '%s' "$out"
}

printf '\n%szaf%s status  %sways · building %s0.5.2%s\n\n' "$Z" "$R" "$D" "$N" "$R"

# 1. what waits on you, always first
printf '%s? 5 questions wait on you%s\n\n' "$Q" "$R"
question() {  # question <id> <on> <text>
  printf '  %s#%s%s %son #%s%s  %s\n' "$QL" "$1" "$R" "$D" "$2" "$R" "$3"
  printf '       %sanswer:%s %szaf%s%s board ruling %s '"'"'…'"'"'%s\n' "$D" "$R" "$Z" "$R" "$V" "$1" "$R"
}
question 272 238 'Should #238 wait on #237 instead of #233, so the script tasks land in order?'
question 278 235 'May the Vcs port gain a tag listing, or do versions come from release folders?'
question 284 239 'Does #239 extend the Forge port itself, or does a separate task do it first?'
question 285 228 'With no terminal, does zaf update change nothing, or update unasked?'
question 286 228 '#280 says Blocked by: #278 by mistake; will you correct it to #279?'
printf '  %sEach whole question: zaf status --more%s\n\n' "$D" "$R"

# 2. trouble, only when there is some
printf '%s! 1 problem%s\n\n' "$BAD" "$R"
printf '  %s#230%s %sorphaned:%s its instance is gone and it still holds the claim\n' "$N" "$R" "$BAD" "$R"
printf '       %sfree it:%s %szaf%s%s spawn stop 230%s\n\n' "$D" "$R" "$Z" "$R" "$V" "$R"

# 3. the version being built, epic by epic
printf '%sEpics of 0.5.2%s\n\n' "$B" "$R"
epic() {  # epic <id> <title> <done> <total> <stage>
  local colour=$D; [ "$5" = done ] && colour=$OK
  printf '  %s#%s%s %s %s%*s %s%s%s  %s%s%s\n' "$N" "$1" "$R" "$(pad "$2" 26)" "$(bar "$3" "$4")" $(( 13 - $4 )) '' "$D" "$(pad "$3/$4" 5)" "$R" "$colour" "$5" "$R"
}
epic 224 'the screens and setup'   0 10 challenged
epic 225 'the install script'      0 7  challenged
epic 226 'begin and the build'     0 13 planned
epic 227 'the live build view'     0 9  challenged
epic 228 'update updates all'      0 10 planned
printf '\n'

# 4. what is moving now
printf '%sNow%s\n\n' "$B" "$R"
printf '  %s⠹%s %schallenger%s on %s#226%s   %sreads the plan for gaps%s\n' "$V" "$R" "$B" "$R" "$N" "$R" "$D" "$R"
printf '  %sready%s    %s4%s  %s#230 #231 #236 #246%s\n' "$OK" "$R" "$B" "$R" "$N" "$R"
printf '  %swaiting%s  %s45%s %son other tickets or on your questions; zaf status --more lists them%s\n\n' "$D" "$R" "$B" "$R" "$D" "$R"

# 5. the machine, one dim line
printf '%s3 of 4 slots free · 1 instance on this machine · no budget cap%s\n' "$D" "$R"
printf '%sNext release: 0.6.0, its design is due: zaf begin --version 0.6.0%s\n\n' "$D" "$R"
