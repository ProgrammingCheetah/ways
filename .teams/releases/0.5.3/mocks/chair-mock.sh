#!/usr/bin/env bash
# Mock of zaf 0.5.3's captain's chair: what `zaf` alone opens in a project, laid out in vim-style splits.
# Everything is pretend: no zaf, no network, nothing written. The bottom bar shows the keys; Ctrl-w moves between panes; q quits.
shopt -s extglob
[[ ${LC_ALL:-${LC_CTYPE:-${LANG:-}}} == *[Uu][Tt][Ff]* ]] || export LC_ALL=C.UTF-8
if ! [ -t 0 ] || ! [ -t 1 ]; then echo "chair-mock needs a terminal" >&2; exit 1; fi

# ── palette ───────────────────────────────────────────────────────────────
E=$'\e'; R="$E[0m"; INV="$E[7m"; B="$E[1m"; UL="$E[4m"
if [ -z "${NO_COLOR:-}" ]; then
  D="$E[2m"; V="$E[1;36m"; N="$E[33m"; OK="$E[32m"; BAD="$E[1;31m"
  Z="$E[1;38;5;201m"; QL="$E[38;5;141m"; Q="$E[1;38;5;141m"
else
  D=; V=; N=; OK=; BAD=; Z=; QL=; Q=
fi
SPIN=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏)

# ── data: ways' board on 2026-11-19, lightly shortened ───────────────────
PROJECTS=(ways dapple zabo); PROJ=0
E_num=(224 225 226 227 228)
E_short=('screens, setup' 'install script' 'begin & build' 'live build view' 'update all')
E_title=('the screens and setup' 'the install script' 'begin and the build start' 'the live build view' 'update updates everything')
E_stage=(challenged challenged planned challenged planned)
E_ids=('230 233 234 235 240 241 243 245 250 252'
       '231 232 237 238 246 248 249'
       '255 257 259 260 261 262 263 264 265 266 267 268 269 292'
       '236 289 239 242 244 247 251 254 256 258'
       '273 274 275 276 277 279 280 281 282 283')
E_obj=("zaf's screens speak to a stranger: one look on every screen, colour that calls the eye, and setup in plain words."
       'One pasted command installs zaf, puts it on PATH and adds the teams plugin.'
       'zaf begin guides a release through its four stages and starts the build.'
       'spawn --watch shows the build as one live view, and a question is answered in it.'
       'zaf update updates everything (binary, plugin, pack, actions) and says what is new.')
E_design=('A screen kit (palette, plan rows, one updating step line). Setup lists its plan, explains each question, and shows only what failed with its fix. The hub draws questions purple (#201).'
          'install.sh names zaf, asks the folder, lists five steps with why, then a line per step. PATH goes in the shell'"'"'s own file once. It ends with the first commands.'
          'Stages are worked out from the release folder and the board. Before a session, zaf says what it is for and how to leave. Sign is [Y/n]. base-open and papers-commit make the integration branch.'
          'One frame redrawn in place, plain stage words per ticket, questions answered in the view; at the end the release pull request (release-open, I12) and the next zaf begin.'
          'A plan first, What is new with its link, then each changed action with why, its diff and Replace it? [Y/n]. With no terminal nothing changes (#285).')
E_signed=(0 0 0 0 0)

declare -A T_state T_title T_blk T_age
tk() { T_title[$1]=$2; T_blk[$1]=$3; T_state[$1]=${4:-w}; T_age[$1]=0; }
tk 230 'screen kit: the palette'          ''              x
tk 233 "a step's line spins"              '230'
tk 234 'plan rows'                        '230'
tk 235 'next version from tags'           ''
tk 240 'setup asks in plain words'        '230'
tk 241 'setup shows only failures'        '230'
tk 243 'setup plan before writing'        '234 240 241'
tk 245 'setup ends with the next step'    '233 235 240 243'
tk 250 'doctor in the screen kit'         '245'
tk 252 'docs: the screens'                '238 248'
tk 231 'install.sh: the first screen'     ''              r
tk 232 'install.sh asks the folder'       '231'
tk 237 'install.sh: PATH once'            '232'
tk 238 'install.sh adds the plugin'       '237'
tk 246 'install.sh downloads zaf'         ''              r
tk 248 'install.sh checks git, claude'    '231'
tk 249 'install.sh on macOS'              '231'
tk 255 'the VCS says if there is a remote' ''
tk 257 "a version's stage worked out"     ''
tk 259 'base-open action'                 ''
tk 260 'papers-commit action'             '259'
tk 261 'build start checks the papers'    '255 259 260'
tk 262 'watch runs the build start'       '261'
tk 263 "hub's start runs the build start" '261'
tk 264 'epic sign asks to build'          '257'
tk 265 'begin draws four stages'          '257'
tk 266 "epics stage: read and sign"       '265'
tk 267 'build stage asks to start'        '265'
tk 268 'built stage: release PR'          '265'
tk 269 'docs: stages, build start'        '255 257 259 260 261 262 263 264 265 266 267 268'
tk 292 'sign with yes as default'         ''
tk 236 "hub's epic names its version"     ''              r
tk 289 'Forge port: link, default branch' ''              r
tk 239 'release-open action'              '289'
tk 242 'stage words in plain words'       ''
tk 244 'the frame fits the window'        '236 242'
tk 247 'watch redraws in place'           '244'
tk 251 'answer in the live view'          '247'
tk 254 'scheduler opens release PR'       '236 239'
tk 256 'view names the release PR'        '247 254'
tk 258 'docs: questions, endings'         '251 254 256'
tk 273 'update: the plan'                 ''
tk 274 'update: the binary'               '230'
tk 275 'update: the plugin'               '230'
tk 276 'update: plugin from the pack'     '275'
tk 277 'update: what is new'              '273'
tk 279 'update: actions, the diff'        ''
tk 280 'update: Replace it? [Y/n]'        '279'
tk 281 'update: one run, everything'      '273 274 276 277 279 280'
tk 282 'update: no terminal, no change'   ''
tk 283 'docs: update'                     '281'

# The inbox: what waits on the Engineer, sorted by how much work it holds.
I_kind=(); I_id=(); I_on=(); I_imp=(); I_text=(); I_rec=(); I_holds=(); I_alive=()
item() { I_kind+=("$1"); I_id+=("$2"); I_on+=("$3"); I_imp+=("$4"); I_text+=("$5"); I_rec+=("$6"); I_holds+=("$7"); I_alive+=(1); }
item phase 0.5.3 '' 99 "0.5.3's objectives are due. Nothing of the Unified UI patch can be designed before they are signed." '' ''
item q 285 228 11 'With no terminal, or inside an instance, should zaf update change nothing after printing its plan, or update the binary and plugin unasked while keeping every differing action?' 'Change nothing: print the plan and exit.' '273 274 275 276 277 279 280 281 282 283'
item q 286 228 11 '#280 was filed with Blocked by: #278 by mistake and left ready; it should read Blocked by: #279 and be blocked. Will you correct its body and label?' 'Yes: Claude corrects #280 to Blocked by: #279.' '273 274 275 276 277 279 280 281 282 283'
item x 230 '' 6 'Orphaned: its instance is gone and it still holds the claim. Six tickets wait on it.' 'Free it: release the claim, and the next tick starts it again.' ''
item q 290 259 6 'base-open opens the integration branch for a version. Should it branch from main, or from the last release tag?' 'From main: the last tag is already on main.' '259'
item q 284 239 4 'release-open must print the link and open against the default branch, but the Forge port has neither. Extend the port in this task, or in a separate task first?' 'A separate task first; #239 waits on it.' '239'
item q 278 235 3 'next_version needs every tag, and the Vcs port only has has_tag. May this task add a tag listing to the port, or should versions come from release folders?' 'Add a tag listing to the Vcs port, with the jj adapter and the fake.' '235'
item q 272 238 1 '#238 is blocked by #233 (the Rust screen kit), but it shares install.sh with #237. Should the blocker be #237?' 'Re-plan: #238 waits on #237, not #233.' '238'
for i in "${!I_kind[@]}"; do
  [ "${I_kind[i]}" = q ] && for t in ${I_holds[i]}; do T_state[$t]=q; done
done
PHASE=objectives   # objectives → design → none


# ── the window tree: splits like vim's, panes of unequal size ────────────
# A node is a leaf (a pane) or a split: v puts its two children side by side, h stacks them.
# N_ratio is the first child's share in percent; Ctrl-w + - < > change it, Ctrl-w = resets it.
N_kind=(v h h h v leaf leaf leaf leaf leaf leaf)
N_ratio=(35 55 45 40 60 0 0 0 0 0 0)
N_a=(1 5 8 6 9); N_b=(2 3 4 7 10)
N_name=('' '' '' '' '' inbox problems now epics detail live)
N_default=("${N_ratio[@]}")
declare -A L_node=([inbox]=5 [problems]=6 [now]=7 [epics]=8 [detail]=9 [live]=10 [claude]=4)
N_parent=(-1 0 0 1 2 1 3 3 2 4 4)
ORDER_PANES=(inbox problems now epics detail live)
declare -A P_top P_left P_h P_w BM
VISIBLE=()
MAX=''             # the maximised pane (Ctrl-w o), or none
TERM_ON=0          # the Claude pane takes the place of detail and live
BCH=([1]='│' [2]='│' [3]='│' [4]='─' [8]='─' [12]='─' [5]='┘' [6]='┐' [9]='└' [10]='┌' [7]='┤' [11]='├' [13]='┴' [14]='┬' [15]='┼')

# ── state of the UI ──────────────────────────────────────────────────────
FNAME=inbox        # the focused pane
QSEL=0; PSEL=0; NSEL=0; ESEL=0; TSEL=0; DSCROLL=0
MODE=normal        # normal | answer | palette | term | signdoc
WPREFIX=0          # Ctrl-w was pressed; the next key is a window command
DMODE=item         # detail: item | prob | epic | read | tree | why | answer
DL=(); DTITLE=''
LIVE_T=231         # the ticket the live pane follows
ABUF=''; AIDX=-1
STATUS=''; STATUS_UNTIL=0
PEND=''; PEND_ARG=''; PEND_END=0; PEND_LEFT=-1
TICK=0; FRAME=0; RESIZED=1
TERM_I=0; TERM_WAIT=0; TERM_LINES=()
PAL_SEL=0
PAL=('Answer the top question|a' 'Start the due phase|o' 'Sign the selected epic|s' 'Dependency tree of the epic|d'
     'Follow the ticket live|l' 'Why is the ticket waiting|w' 'Maximise this pane|^w o' 'Switch project|1' 'Quit (the build goes on)|q')
OUT=''

# ── drawing helpers ──────────────────────────────────────────────────────
vis() { local t=${1//$'\e['*([0-9;])m/}; printf '%s' "$t"; }
# put <row> <col> <width> <text>: draw text padded to width; never touches columns past it.
put() {
  local s=$4 t; t=${4//$'\e['*([0-9;])m/}
  local n=${#t}
  if (( n > $3 )); then s=${t:0:$3}; n=$3; fi
  local pad; printf -v pad '%*s' $(( $3 - n )) ''
  OUT+="$E[$1;$2H$s$R$pad"
}
flush() { printf '%s' "$OUT"; OUT=''; }
trunc() { local s=$1 w=$2; (( w < 1 )) && { printf ''; return; }; (( ${#s} > w )) && s="${s:0:w-1}…"; printf '%s' "$s"; }
wrap() { local w=$1; shift; (( w < 8 )) && w=8; printf '%s\n' "$*" | fold -s -w "$w" | command sed 's/ *$//'; }
shown() { [ -n "${P_h[$1]:-}" ]; }
vlen() { local t=${1//$'\e['*([0-9;])m/}; VL=${#t}; }
truncv() { local s=$1 w=$2; (( w < 1 )) && { TR=''; return; }; (( ${#s} > w )) && s="${s:0:w-1}…"; TR=$s; }
gap_join() {  # gap_join <width> <left> <right>: GP holds them with at least one space between
  local lv rv g; vlen "$2"; lv=$VL; vlen "$3"; rv=$VL
  printf -v g '%*s' $(( $1 - lv - rv > 1 ? $1 - lv - rv : 1 )) ''; GP="$2$R$g$3"
}
nowv() { local t=${EPOCHREALTIME/[.,]/}; NOW=$(( t / 1000 )); }
# pput <pane> <row in pane> <text>
pput() { put $(( P_top[$1] + $2 )) "${P_left[$1]}" "${P_w[$1]}" "$3"; }
# pclear <pane> <from row>: blank the pane's rows from there down
pclear() { local i; for ((i = $2; i < P_h[$1]; i++)); do pput "$1" "$i" ''; done; }
# gapped <width> <left> <right>: left and right text with at least one space between
gapped() { local l r g; l=$(vis "$2"); r=$(vis "$3"); printf -v g '%*s' $(( $1 - ${#l} - ${#r} > 1 ? $1 - ${#l} - ${#r} : 1 )) ''; printf '%s%s%s' "$2$R" "$g" "$3"; }

screen_size() {
  local size; size=$(stty size 2>/dev/null); ROWS=${size% *}; COLS=${size#* }
  [[ $ROWS =~ ^[0-9]+$ ]] && (( ROWS > 0 )) || ROWS=24
  [[ $COLS =~ ^[0-9]+$ ]] && (( COLS > 0 )) || COLS=80
  TOP=3; BOT=$((ROWS - 2))
}

# layout: the tree's rectangles and the shared border cells (BM: up 1, down 2, left 4, right 8)
mark() { (( $1 >= 2 && $1 <= BOT && $2 >= 1 && $2 <= COLS )) && BM[$1,$2]=$(( ${BM[$1,$2]:-0} | $3 )); }
vsep() { local r; for ((r = $2; r < $2 + $3; r++)); do mark $r "$1" 3; done; mark $(( $2 - 1 )) "$1" 2; mark $(( $2 + $3 )) "$1" 1; }
hsep() { local c; for ((c = $2; c < $2 + $3; c++)); do mark "$1" $c 12; done; mark "$1" $(( $2 - 1 )) 8; mark "$1" $(( $2 + $3 )) 4; }
leaf() { P_top[$1]=$2; P_left[$1]=$3; P_h[$1]=$4; P_w[$1]=$5; VISIBLE+=("$1"); }
layout_node() {  # layout_node <node> <top> <left> <height> <width>
  local n=$1 t=$2 l=$3 h=$4 w=$5
  if (( n == 4 && TERM_ON )); then leaf claude "$t" "$l" "$h" "$w"; return; fi
  if [ "${N_kind[n]}" = leaf ]; then leaf "${N_name[n]}" "$t" "$l" "$h" "$w"; return; fi
  local r=${N_ratio[n]}
  if [ "${N_kind[n]}" = v ]; then
    local wa=$(( (w - 1) * r / 100 )); local wb=$(( w - 1 - wa ))
    vsep $(( l + wa )) "$t" "$h"
    layout_node "${N_a[n]}" "$t" "$l" "$h" "$wa"
    layout_node "${N_b[n]}" "$t" $(( l + wa + 1 )) "$h" "$wb"
  else
    local ha=$(( (h - 1) * r / 100 )); local hb=$(( h - 1 - ha ))
    hsep $(( t + ha )) "$l" "$w"
    layout_node "${N_a[n]}" "$t" "$l" "$ha" "$w"
    layout_node "${N_b[n]}" $(( t + ha + 1 )) "$l" "$hb" "$w"
  fi
}
relayout() {
  P_top=(); P_left=(); P_h=(); P_w=(); BM=(); VISIBLE=()
  local c; for ((c = 1; c <= COLS; c++)); do BM[2,$c]=12; done   # the rule under the tabs
  if [ -n "$MAX" ]; then leaf "$MAX" "$TOP" 1 $(( BOT - TOP + 1 )) "$COLS"
  else layout_node 0 "$TOP" 1 $(( BOT - TOP + 1 )) "$COLS"; fi
  shown "$FNAME" || FNAME=${VISIBLE[0]}
}
sizes_ok() { local p; for p in "${VISIBLE[@]}"; do (( P_h[$p] >= 3 && P_w[$p] >= 16 )) || return 1; done; }
draw_borders() {
  local k ch
  for k in "${!BM[@]}"; do ch=${BCH[${BM[$k]}]:-─}; OUT+="$E[${k%,*};${k#*,}H$D$ch$R"; done
}

glyph() {  # glyph <ticket>: sets G to the ticket's coloured symbol
  case ${T_state[$1]} in
    d) G="$OK■" ;; r) G="$V${SPIN[FRAME % 10]}" ;; q) G="$QL?" ;; x) G="$BAD✗" ;; *) G="$D□" ;;
  esac
}
word() {
  case ${T_state[$1]} in
    d) echo done ;; r) echo running ;; q) echo 'waits on you' ;; x) echo orphaned ;; *) echo waiting ;;
  esac
}
running() { local n=0 t; for t in "${!T_state[@]}"; do [ "${T_state[$t]}" = r ] && n=$((n + 1)); done; echo $n; }
RUNNING=()
running_list() { RUNNING=(); local t; for t in ${E_ids[*]}; do [ "${T_state[$t]}" = r ] && RUNNING+=("$t"); done; }
# the inbox (phases and questions) and the problems, each most held work first
sorted() {  # sorted <kinds…>: SORTED holds the live items of those kinds
  SORTED=(); local i j k
  for i in "${!I_kind[@]}"; do (( I_alive[i] )) && [[ " $* " == *" ${I_kind[i]} "* ]] && SORTED+=("$i"); done
  for ((i = 0; i < ${#SORTED[@]}; i++)); do
    for ((j = i + 1; j < ${#SORTED[@]}; j++)); do
      if (( I_imp[SORTED[j]] > I_imp[SORTED[i]] )); then k=${SORTED[i]}; SORTED[i]=${SORTED[j]}; SORTED[j]=$k; fi
    done
  done
}
inbox_items() { sorted phase q; INBOX=("${SORTED[@]}"); }
problem_items() { sorted x; PROBS=("${SORTED[@]}"); }

title() {  # title <pane> <text> [right]: TT holds the title, cyan with a bar when the pane is focused
  local t lv
  if [ "$FNAME" = "$1" ] && { [ "$MODE" = normal ] || [ "$MODE" = term ]; }; then t="${V}▌ $2"; else t="${B}  $2"; fi
  TT=$t
  [ -n "${3:-}" ] || return 0
  vlen "$t"; lv=$VL; vlen "$3 "
  (( lv + 1 + VL <= P_w[$1] )) && { gap_join "${P_w[$1]}" "$t" "$3 "; TT=$GP; }
  return 0
}

# ── panes ────────────────────────────────────────────────────────────────
draw_tabs() {
  local s=" ${Z}zaf${R}  " i badge right n
  inbox_items; problem_items; n=$(( ${#INBOX[@]} + ${#PROBS[@]} ))
  for i in 0 1 2; do
    badge=''; (( i == 0 && n > 0 )) && badge=" ${Q}?${n}${R}"
    if (( i == PROJ )); then s+="${B}${UL}${PROJECTS[i]}${R}${badge}   "
    else s+="${D}${PROJECTS[i]}${R}${badge}   "; fi
  done
  [ -n "$MAX" ] && s+="${INV} max: ${MAX} ${R}"
  if (( PROJ == 0 )); then
    running_list
    right="${N}0.5.2${R} ${V}${SPIN[FRAME % 10]}${R} building ${D}·${R} ${#RUNNING[@]} running "
  else right="${D}nothing building ${R}"; fi
  gap_join "$COLS" "$s" "$right"; put 1 1 "$COLS" "$GP"
}

draw_inbox() {
  shown inbox || return
  inbox_items; local n=${#INBOX[@]} w=${P_w[inbox]} row=1 k i
  (( PROJ != 0 )) && n=0
  title inbox "Waiting on you ${Q}${n}" "${D}by work held"; pput inbox 0 "$TT"
  if (( PROJ != 0 || n == 0 )); then pput inbox 1 "  ${OK}✓${R} nothing waits on you"; pclear inbox 2; return; fi
  (( QSEL >= n )) && QSEL=$(( n - 1 ))
  local first=0; (( QSEL >= P_h[inbox] - 1 )) && first=$(( QSEL - P_h[inbox] + 2 ))
  for ((k = first; k < n && row < P_h[inbox]; k++)); do
    i=${INBOX[k]}
    local mark=' ' head text
    [ "$FNAME" = inbox ] && (( k == QSEL )) && mark="${V}▸"
    if [ "${I_kind[i]}" = phase ]; then head="${V}▶ ${I_id[i]} $PHASE due"; text=''
    else head="${Q}?${R} ${QL}#${I_id[i]}${R} ${D}$(printf '%2s' "${I_imp[i]}")"; text=${I_text[i]}; fi
    local hv; hv=$(vis "$mark $head ")
    pput inbox $row "$mark$R $head$R ${D}$(trunc "$text" $(( w - ${#hv} )))"
    row=$((row + 1))
  done
  pclear inbox $row
}

draw_problems() {
  shown problems || return
  problem_items; local n=${#PROBS[@]} row=1 k i
  (( PROJ != 0 )) && n=0
  local count="${OK}0"; (( n )) && count="${BAD}$n"
  title problems "Problems $count"; pput problems 0 "$TT"
  if (( PROJ != 0 || n == 0 )); then pput problems 1 "  ${OK}✓${R} nothing is wrong"; pclear problems 2; return; fi
  (( PSEL >= n )) && PSEL=$(( n - 1 ))
  for ((k = 0; k < n && row < P_h[problems]; k++)); do
    i=${PROBS[k]}; local mark=' '
    [ "$FNAME" = problems ] && (( k == PSEL )) && mark="${V}▸"
    pput problems $row "$mark$R ${BAD}✗${R} ${N}#${I_id[i]}${R} ${BAD}orphaned${R} ${D}holds ${I_imp[i]}"
    row=$((row + 1))
  done
  (( row < P_h[problems] )) && { pput problems $row "  ${D}Enter frees it"; row=$((row + 1)); }
  pclear problems $row
}

draw_now() {
  shown now || return
  local run row=1 k t w=${P_w[now]}
  running_list; run=${#RUNNING[@]}
  if (( PROJ == 0 )); then title now 'Now' "${D}$run of 4 slots"; else title now 'Now'; fi; pput now 0 "$TT"
  if (( PROJ != 0 )); then pput now 1 "  ${D}nothing running"; pclear now 2; return; fi
  (( NSEL >= run )) && NSEL=$(( run > 0 ? run - 1 : 0 ))
  for ((k = 0; k < run && row < P_h[now] - 1; k++)); do
    t=${RUNNING[k]}; local mark=' '
    [ "$FNAME" = now ] && (( k == NSEL )) && mark="${V}▸"
    truncv "${T_title[$t]}" $(( w - 10 )); pput now $row "$mark$R ${V}${SPIN[FRAME % 10]}${R} ${N}#$t${R} $TR"
    row=$((row + 1))
  done
  local ready=0 waiting=0
  for t in ${E_ids[*]}; do
    case ${T_state[$t]} in
      w) local ok=1 b; for b in ${T_blk[$t]}; do [ -n "${T_state[$b]:-}" ] && [ "${T_state[$b]}" != d ] && ok=0; done
         if (( ok )); then ready=$((ready + 1)); else waiting=$((waiting + 1)); fi ;;
      q|x) waiting=$((waiting + 1)) ;;
    esac
  done
  (( row < P_h[now] )) && { pput now $row "  ${OK}ready ${ready}${R}  ${D}waiting ${waiting}"; row=$((row + 1)); }
  pclear now $row
}

draw_epics() {
  shown epics || return
  local w=${P_w[epics]} row=1 e
  if (( PROJ == 0 )); then title epics 'Epics of 0.5.2' "${D}squares: one per ticket"; else title epics 'Epics'; fi; pput epics 0 "$TT"
  if (( PROJ != 0 )); then
    pput epics 1 "  ${D}${PROJECTS[PROJ]}: no version is building"
    pput epics 2 "  ${D}start one: ${R}${Z}zaf${R}${V} begin"; pclear epics 3; return
  fi
  for e in 0 1 2 3 4; do
    (( row >= P_h[epics] - 1 )) && break
    local ids=(${E_ids[e]}) done=0 t sq='' k=0 mark=' ' stage=${E_stage[e]}
    for t in "${ids[@]}"; do
      [ "${T_state[$t]}" = d ] && done=$((done + 1))
      glyph "$t"
      if (( e == ESEL && k == TSEL )) && [ "$FNAME" = epics ] && [ "$MODE" = normal ]; then sq+="${INV}${G}${R}"
      else sq+="${G}${R}"; fi
      k=$((k + 1))
    done
    (( e == ESEL )) && mark="${V}▸"
    (( E_signed[e] )) && stage="${OK}✓ signed"
    [ "$PEND" = sign ] && [ "$PEND_ARG" = "$e" ] && stage="${QL}signing…"
    local gap name cnt lv; printf -v gap '%*s' $(( 15 - ${#ids[@]} )) ''
    printf -v name '%-15s' "${E_short[e]}"; printf -v cnt '%5s' "$done/${#ids[@]}"
    local line="$mark$R ${N}#${E_num[e]}${R} $name $sq$gap ${D}$cnt"
    vlen "$line"; lv=$VL; vlen "$stage"
    (( lv + 2 + VL <= w )) && line+="  ${D}$stage"
    pput epics $row "$line"
    row=$((row + 1))
  done
  pclear epics $row
  if (( P_h[epics] >= 8 )); then
    local sp=${SPIN[FRAME % 10]}
    if (( P_w[epics] >= 51 )); then pput epics $(( P_h[epics] - 1 )) "  ${OK}■${R}${D} done  ${R}${V}$sp${R}${D} running  □ waiting  ${R}${QL}?${R}${D} yours  ${R}${BAD}✗${R}${D} problem"
    else pput epics $(( P_h[epics] - 1 )) "  ${OK}■${R}${D}done ${R}${V}$sp${R}${D}run □wait ${R}${QL}?${R}${D}you ${R}${BAD}✗${R}${D}fix"; fi
  fi
}

draw_detail() {
  shown detail || return
  detail_build
  local row=1 i
  title detail "$DTITLE"; pput detail 0 "$TT"
  for ((i = DSCROLL; i < ${#DL[@]} && row < P_h[detail]; i++)); do pput detail $row " ${DL[i]}"; row=$((row + 1)); done
  pclear detail $row
}

LIVE=('● Read the brief pack' '● Read zaf/src/app/begin.rs' '● Edit begin.rs: the prompt' '● Run cargo test begin::'
      '  ✓ 41 passed' '● Edit tests.rs: Enter signs' '● Run the gate' '  ✓ gate green' '● Commit the change' '● Open the pull request')
draw_live() {
  shown live || return
  local t=$LIVE_T w=${P_w[live]} row=1
  if [ "${T_state[$t]:-}" != r ]; then running_list; (( ${#RUNNING[@]} )) && { LIVE_T=${RUNNING[0]}; t=$LIVE_T; }; fi
  local st=''; [ "${T_state[$t]:-}" = r ] && st=running
  if (( PROJ == 0 )); then title live "Live ${N}#$t" "${D}$st"; else title live 'Live'; fi; pput live 0 "$TT"
  if (( PROJ != 0 )) || [ "${T_state[$t]:-}" != r ]; then pput live 1 "  ${D}nothing running"; pclear live 2; return; fi
  local start=$(( t % ${#LIVE[@]} )) n=$(( T_age[$t] * 2 + 2 )) k lines=()
  (( n > ${#LIVE[@]} )) && n=${#LIVE[@]}
  for ((k = 0; k < n; k++)); do lines+=("${LIVE[(start + k) % ${#LIVE[@]}]}"); done
  local room=$(( P_h[live] - 2 )) from=0
  (( ${#lines[@]} > room )) && from=$(( ${#lines[@]} - room ))
  for ((k = from; k < ${#lines[@]}; k++)); do truncv "${lines[k]}" $(( w - 2 )); pput live $row " $TR"; row=$((row + 1)); done
  (( row < P_h[live] )) && { truncv "${T_title[$t]}" $(( w - 4 )); pput live $row " ${V}${SPIN[FRAME % 10]}${R} ${D}$TR"; row=$((row + 1)); }
  pclear live $row
}

draw_status() {
  local s=$STATUS
  [ -n "$s" ] || s="${D}Closing zaf never stops the build · ${R}${V}:${R}${D} actions · ${R}${V}^w${R}${D} windows · ${R}${V}1 2 3${R}${D} projects"
  put $((ROWS - 1)) 1 "$COLS" " $s"
}

keys() {  # the bottom bar: the keys of what is focused
  local k
  if (( WPREFIX )); then k='h/j/k/l|focus;+ -|height;< >|width;=|equal;o|maximise;Esc|cancel'
  else case $MODE in
    answer)  k='type|edit;Enter|rule;Ctrl-U|clear;Esc|cancel' ;;
    palette) k='j/k|move;Enter|run;Esc|close' ;;
    term)    k='Claude runs in this pane ·;Esc|leave Claude' ;;
    signdoc) k='Enter|sign;n|leave unsigned' ;;
    *) case $FNAME in
         inbox)    k='j/k|move;Enter|answer;o|start phase;Tab|next pane;q|quit' ;;
         problems) k='j/k|move;Enter|free it;Tab|next pane;q|quit' ;;
         now)      k='j/k|move;l|follow live;w|why;Tab|next pane;q|quit' ;;
         epics)    k='j/k|epic;←→|ticket;Enter|read;s|sign;d|tree;l|live;w|why;q|quit' ;;
         detail)   k='j/k|scroll;Esc|back;Tab|next pane;q|quit' ;;
         live)     k='Tab|next pane;q|quit' ;;
       esac ;;
  esac; fi
  [ -n "$PEND" ] && (( PEND_LEFT >= 0 )) && k="u|undo;$k"
  local s=' ' pair key desc IFS=';'
  for pair in $k; do
    if [[ $pair == *'|'* ]]; then key=${pair%%|*}; desc=${pair#*|}; s+="${V}${key}${R} ${D}${desc}${R}  "
    else s+="${D}${pair} ${R}"; fi
  done
  put "$ROWS" 1 "$COLS" "$s"
}

draw_pane() {
  case $1 in
    inbox) draw_inbox ;; problems) draw_problems ;; now) draw_now ;; epics) draw_epics ;;
    detail) draw_detail ;; live) draw_live ;; claude) draw_claude ;;
  esac
}
# redraw_all: every cell of the screen once, in one write; Ctrl-w and the palette use it, so nothing flickers
redraw_all() {
  if (( COLS < 80 || ROWS < 24 )); then
    OUT+="$E[2J$E[1;1H zaf needs at least 80×24; this terminal is ${COLS}×${ROWS}."; flush; return
  fi
  local p
  draw_tabs; draw_borders
  for p in "${VISIBLE[@]}"; do draw_pane "$p"; done
  draw_status; keys
  [ "$MODE" = palette ] && draw_palette
  flush
}

# ── the detail pane's contents ───────────────────────────────────────────
dadd() { local colour=$1; shift; local line; while IFS= read -r line; do DL+=("$colour$line"); done < <(wrap $(( P_w[detail] - 2 )) "$*"); }
dblank() { DL+=(''); }
sel_ticket() { local ids=(${E_ids[ESEL]}); echo "${ids[TSEL]}"; }
WHY_T=''

detail_build() {
  DL=()
  if (( PROJ != 0 )); then DTITLE=${PROJECTS[PROJ]}; dadd "$D" 'Last release done. Nothing waits on you here.'; dblank; dadd '' 'Start the next one: zaf begin'; return; fi
  case $DMODE in
    item)   detail_item ;;
    prob)   detail_prob ;;
    epic)   detail_epic 0 ;;
    read)   detail_epic 1 ;;
    tree)   detail_tree ;;
    why)    detail_why ;;
    answer) detail_answer ;;
  esac
}

detail_item() {
  inbox_items
  if (( ${#INBOX[@]} == 0 )); then DTITLE='All clear'; dadd "$OK" '✓ Nothing waits on you. The build goes on.'; return; fi
  (( QSEL >= ${#INBOX[@]} )) && QSEL=$(( ${#INBOX[@]} - 1 ))
  local i=${INBOX[QSEL]}
  case ${I_kind[i]} in
    phase) DTITLE="Release ${I_id[i]}"
           dadd '' "${I_text[i]/objectives/$PHASE}"; dblank
           dadd "$D" 'Claude asks, you answer in your own words, and you sign when the file says what you mean.'; dblank
           DL+=("${INV}${V} o ${R} ${B}Start $PHASE${R}") ;;
    q)     DTITLE="Question #${I_id[i]} on #${I_on[i]}"
           dadd '' "${I_text[i]}"; dblank
           DL+=("${D}Claude's answer:"); dadd "$QL" "${I_rec[i]}"; dblank
           dadd "$D" "Holds ${I_imp[i]} tickets: $(printf '#%s ' ${I_holds[i]})"; dblank
           DL+=("${INV}${V} Enter ${R} ${B}answer${R}") ;;
  esac
}
detail_prob() {
  problem_items
  if (( ${#PROBS[@]} == 0 )); then DTITLE='No problems'; dadd "$OK" '✓ Nothing is wrong.'; return; fi
  local i=${PROBS[PSEL]}
  DTITLE="Problem #${I_id[i]}"
  dadd '' "${I_text[i]}"; dblank; dadd "$D" "${I_rec[i]}"; dblank
  DL+=("${INV}${V} Enter ${R} ${B}free it${R}")
}

detail_epic() {  # detail_epic <full>: a short look, or the objectives and the design
  local e=$ESEL ids=(${E_ids[ESEL]}) t d=0 r=0 q=0 w=0
  for t in "${ids[@]}"; do case ${T_state[$t]} in d) d=$((d+1));; r) r=$((r+1));; q) q=$((q+1));; *) w=$((w+1));; esac; done
  DTITLE="Epic #${E_num[e]}"
  dadd "$B" "${E_title[e]}"
  DL+=("${OK}■ $d done${R}  ${V}${SPIN[FRAME % 10]} $r${R}  ${QL}? $q${R}  ${D}□ $w")
  dblank
  if (( $1 )); then
    DL+=("${B}Objectives"); dadd '' "${E_obj[e]}"; dblank
    DL+=("${B}Design"); dadd '' "${E_design[e]}"; dblank
  else
    dadd "$D" 'Enter reads its objectives and design.'; dblank
  fi
  if (( E_signed[e] )); then DL+=("${OK}✓ signed")
  else DL+=("${INV}${V} s ${R} ${B}Sign epic${R}"); DL+=("${D}d tree · w why · Esc back"); fi
}

tree_rows() {  # tree_rows <parent> <prefix>: the tickets whose first blocker inside the epic is <parent>
  local parent=$1 prefix=$2 kids=() t first b
  for t in ${E_ids[ESEL]}; do
    first=''
    for b in ${T_blk[$t]}; do [[ " ${E_ids[ESEL]} " == *" $b "* ]] && { first=$b; break; }; done
    [ "$first" = "$parent" ] && kids+=("$t")
  done
  local n=${#kids[@]} k branch next extra
  for ((k = 0; k < n; k++)); do
    t=${kids[k]}
    if (( k == n - 1 )); then branch='└─'; next="$prefix   "; else branch='├─'; next="$prefix│  "; fi
    local more=0; for b in ${T_blk[$t]}; do more=$((more + 1)); done
    extra=''; (( more > 1 )) && extra=" ${D}+$((more - 1))"
    glyph "$t"; DL+=("${D}${prefix}${branch}${R}${G}${R} ${N}#$t${R} $(trunc "${T_title[$t]}" $(( P_w[detail] - ${#prefix} - 12 )))$extra")
    tree_rows "$t" "$next"
  done
}
detail_tree() {
  DTITLE="Tree #${E_num[ESEL]}"
  dadd "$B" "${E_title[ESEL]}"
  tree_rows '' ''
  dblank; dadd "$D" '+n: more blockers than the one drawn. d closes the tree.'
}

detail_why() {
  local t=$WHY_T b
  DTITLE="Why #$t"
  dadd "$B" "${T_title[$t]}"
  glyph "$t"; DL+=("${G}${R} $(word "$t")"); dblank
  case ${T_state[$t]} in
    d) dadd '' 'Done: merged into feature/0.5.2.' ;;
    r) dadd '' 'Running: implement, then ci, review and merge. l follows it live.' ;;
    x) dadd '' 'Its instance is gone and it still holds the claim. Free it from Problems.' ;;
    q) local i; for i in "${!I_kind[@]}"; do
         (( I_alive[i] )) && [[ " ${I_holds[i]} " == *" $t "* ]] && dadd "$QL" "Held by your question #${I_id[i]}: answer it in the inbox."
       done ;;
    *) local open=0
       for b in ${T_blk[$t]}; do
         [ -n "${T_state[$b]:-}" ] && [ "${T_state[$b]}" != d ] && { glyph "$b"; DL+=("waits on ${G}${R} ${N}#$b${R} ${D}$(word "$b")"); open=1; }
       done
       (( open )) || dadd '' "Ready: it starts when a slot frees ($(running) of 4 busy)." ;;
  esac
  dblank
  dadd "$D" 'next: implement → ci → review → merge'
  dadd "$D" "so far: $(( T_age[$t] * 4 )) min · $(( T_age[$t] * 18 ))k tokens"
}

detail_answer() {
  local i=$AIDX
  DTITLE="Answer #${I_id[i]}"
  dadd "$B" "Your ruling (Claude's answer, yours to change)"
  local w=$(( P_w[detail] - 4 )) line gap
  DL+=("${D}┌$(printf '─%.0s' $(seq 1 $w))┐")
  while IFS= read -r line; do printf -v gap '%*s' $(( w - ${#line} > 0 ? w - ${#line} : 0 )) ''; DL+=("${D}│${R}${line}${gap}${D}│"); done < <(wrap "$w" "${ABUF}▏")
  DL+=("${D}└$(printf '─%.0s' $(seq 1 $w))┘")
  dadd "$D" 'Enter rules it · Esc cancels'; dblank
  dadd "$D" "${I_text[i]}"
}

# ── the Claude pane: a split in the place of detail and live ─────────────
TERM_SCRIPT_objectives=('Claude  What is 0.5.3 for, in one sentence?'
  'you     The Unified UI: I never leave zaf, not even for a browser.'
  'Claude  Who is it for, and how would you check it by hand?'
  'you     Me first. I check nothing outside zaf for a whole release.'
  'Claude  Writing objectives.md …'
  'Claude  ✓ objectives.md written.'
  'Claude  Ready for Exit.')
TERM_SCRIPT_design=('Claude  I read the signed objectives. Two forks for you.'
  'Claude  1. ratatui with crossterm: settled. 2. Claude in a pane: tui-term.'
  'you     Agreed on both.'
  'Claude  Writing design.md …'
  'Claude  ✓ design.md written.'
  'Claude  Ready for Exit.')
draw_claude() {
  shown claude || return
  local w=${P_w[claude]} rows=() src part first i row=1 line
  title claude "claude ${D}· $PHASE 0.5.3 · inside zaf" "${D}Esc leaves"; pput claude 0 "$TT"
  for src in "${TERM_LINES[@]}"; do
    first=1
    while IFS= read -r part; do
      if (( first )); then rows+=("$part"); first=0; else rows+=("        $part"); fi
    done < <(wrap $(( w - 10 )) "$src")
  done
  local from=0; (( ${#rows[@]} > P_h[claude] - 1 )) && from=$(( ${#rows[@]} - P_h[claude] + 1 ))   # a terminal: the newest lines show
  for ((i = from; i < ${#rows[@]} && row < P_h[claude]; i++)); do
    line=${rows[i]}
    case $line in Claude*) line="${Z}Claude${R}${line#Claude}" ;; you*) line="${V}you${R}${line#you}" ;; esac
    [[ $line == *'Ready for Exit.'* ]] && line="${line/Ready for Exit./${OK}${B}Ready for Exit.${R}}"
    pput claude $row " $line"; row=$((row + 1))
  done
  pclear claude $row
}

draw_palette() {
  local w=40 x=$(( (COLS - 40) / 2 + 1 )) y=$(( TOP + 2 )) k
  put $y "$x" $w "${V}┌─ ${B}actions${R}${V} $(printf '─%.0s' $(seq 1 $(( w - 13 ))))┐"
  for ((k = 0; k < ${#PAL[@]}; k++)); do
    local name=${PAL[k]%%|*} key=${PAL[k]#*|} line
    line=$(printf ' %-28s %7s ' "$name" "$key")
    if (( k == PAL_SEL )); then put $((y + 1 + k)) "$x" $w "${V}│${R}${INV}${line}${R}${V}│"
    else put $((y + 1 + k)) "$x" $w "${V}│${R}${line}${V}│"; fi
  done
  put $((y + ${#PAL[@]} + 1)) "$x" $w "${V}└$(printf '─%.0s' $(seq 1 $(( w - 2 ))))┘"
}

# ── the pretend build ────────────────────────────────────────────────────
held() { local i; for i in "${!I_kind[@]}"; do (( I_alive[i] )) && [ "${I_kind[i]}" = q ] && [[ " ${I_holds[i]} " == *" $1 "* ]] && return 0; done; return 1; }
tick() {
  TICK=$((TICK + 1))
  local t b ok run
  for t in "${!T_state[@]}"; do
    [ "${T_state[$t]}" = r ] || continue
    T_age[$t]=$(( T_age[$t] + 1 ))
    (( T_age[$t] >= 3 + t % 3 )) && T_state[$t]=d
  done
  run=$(running)
  for t in ${E_ids[*]}; do
    (( run >= 4 )) && break
    [ "${T_state[$t]}" = w ] || continue
    held "$t" && continue
    ok=1; for b in ${T_blk[$t]}; do [ -n "${T_state[$b]:-}" ] && [ "${T_state[$b]}" != d ] && ok=0; done
    (( ok )) && { T_state[$t]=r; run=$((run + 1)); }
  done
  if (( TICK == 4 )); then   # a new question arrives: bell, inbox, square
    item q 293 246 2 'install.sh asks where to install zaf. Should the answer accept ~ and $HOME and expand them?' 'Yes: expand ~ and $HOME, and refuse a folder that is not writable.' '246'
    T_state[246]=q
    OUT+=$'\a'
    note "${Q}? New question #293${R} on #246 · it is in your inbox"
    [ "$MODE" = palette ] || draw_inbox
  fi
  [ "$MODE" = palette ] && return
  draw_tabs; draw_epics; draw_now
  case $DMODE in why|epic|read|tree) [ "$MODE" = term ] || draw_detail ;; esac
}

note() { STATUS=$1; STATUS_UNTIL=$(( SECONDS + 6 )); draw_status; }

# ── undo: a ruling or a signature waits five seconds before it counts ────
pend() { PEND=$1; PEND_ARG=$2; PEND_END=$(( SECONDS + 5 )); PEND_LEFT=-2; pend_status; keys; }
pend_status() {
  local left=$(( PEND_END - SECONDS )); (( left < 0 )) && left=0
  (( left == PEND_LEFT )) && return
  PEND_LEFT=$left
  case $PEND in
    rule) STATUS="${OK}✓${R} #${I_id[PEND_ARG]} ruled ${D}·${R} ${V}u${R} to undo ${D}(${left}s)" ;;
    sign) STATUS="${OK}✓${R} epic #${E_num[PEND_ARG]} signed ${D}·${R} ${V}u${R} to undo ${D}(${left}s)" ;;
    doc)  STATUS="${OK}✓${R} ${PEND_ARG}.md signed ${D}·${R} ${V}u${R} to undo ${D}(${left}s)" ;;
  esac
  STATUS_UNTIL=$(( SECONDS + 99 )); draw_status
}
pend_check() {
  [ -n "$PEND" ] || return
  if (( SECONDS < PEND_END )); then pend_status; return; fi
  local t i=$PEND_ARG
  case $PEND in
    rule) for t in ${I_holds[i]}; do held "$t" || { [ "${T_state[$t]}" = q ] && T_state[$t]=w; }; done
          note "${OK}✓${R} #${I_id[i]} ruled ${D}· its tickets go on" ;;
    sign) E_signed[i]=1; E_stage[i]=signed; note "${OK}✓${R} epic #${E_num[i]} signed ${D}· zaf plans its tickets" ;;
    doc)  note "${OK}✓${R} ${i}.md signed ${D}· the ${i} stage is done" ;;
  esac
  PEND=''; PEND_LEFT=-1; keys
  [ "$MODE" = palette ] || { draw_inbox; draw_epics; draw_detail; }
}
undo() {
  [ -n "$PEND" ] || return
  case $PEND in
    rule) I_alive[PEND_ARG]=1; note "↶ #${I_id[PEND_ARG]} is back in your inbox" ;;
    sign) note "↶ epic #${E_num[PEND_ARG]} not signed" ;;
    doc)  PHASE=$PEND_ARG; I_alive[0]=1; note "↶ ${PEND_ARG}.md not signed; o opens it again" ;;
  esac
  PEND=''; PEND_LEFT=-1; keys; draw_inbox; draw_tabs; draw_epics; draw_detail
}

# ── windows: focus by direction, resize, maximise ────────────────────────
set_focus() {  # set_focus <pane>: the old and the new pane redraw for their titles
  local old=$FNAME; FNAME=$1
  case $FNAME in
    inbox) DMODE=item; DSCROLL=0 ;; problems) DMODE=prob; DSCROLL=0 ;;
    epics) [ "$DMODE" = tree ] || [ "$DMODE" = read ] || DMODE=epic ;;
  esac
  draw_pane "$old"; draw_pane "$FNAME"; draw_detail; keys
}
focus_dir() {  # focus_dir <h|j|k|l>: the nearest pane that way, nearest to the focused pane's middle
  local f=$FNAME best='' bestd=999999 p
  local ft=${P_top[$f]} fl=${P_left[$f]} fh=${P_h[$f]} fw=${P_w[$f]}
  local mr=$(( ft + fh / 2 )) mc=$(( fl + fw / 2 ))
  for p in "${VISIBLE[@]}"; do
    [ "$p" = "$f" ] && continue
    local t=${P_top[$p]} l=${P_left[$p]} h=${P_h[$p]} w=${P_w[$p]} along perp
    case $1 in
      h) (( l + w <= fl )) || continue; along=$(( fl - l - w )) ;;
      l) (( l >= fl + fw )) || continue; along=$(( l - fl - fw )) ;;
      k) (( t + h <= ft )) || continue; along=$(( ft - t - h )) ;;
      j) (( t >= ft + fh )) || continue; along=$(( t - ft - fh )) ;;
    esac
    if [[ $1 == [hl] ]]; then
      if (( mr < t )); then perp=$(( t - mr )); elif (( mr > t + h - 1 )); then perp=$(( mr - t - h + 1 )); else perp=0; fi
    else
      if (( mc < l )); then perp=$(( l - mc )); elif (( mc > l + w - 1 )); then perp=$(( mc - l - w + 1 )); else perp=0; fi
    fi
    (( along * 100 + perp < bestd )) && { bestd=$(( along * 100 + perp )); best=$p; }
  done
  [ -n "$best" ] && set_focus "$best"
}
resize_split() {  # resize_split <h|v> <delta>: the nearest split of that kind above the focused pane
  local child=${L_node[$FNAME]} p
  p=${N_parent[child]}
  while (( p >= 0 )) && [ "${N_kind[p]}" != "$1" ]; do child=$p; p=${N_parent[p]}; done
  (( p < 0 )) && { note "${D}no split to resize that way"; return; }
  local old=${N_ratio[p]} new
  if [ "${N_a[p]}" = "$child" ]; then new=$(( old + $2 )); else new=$(( old - $2 )); fi
  (( new < 10 )) && new=10; (( new > 90 )) && new=90
  N_ratio[p]=$new; relayout
  if ! sizes_ok; then N_ratio[p]=$old; relayout; note "${D}that pane cannot get smaller"; fi
  redraw_all
}
window_key() {
  WPREFIX=0
  [ "$MODE" = normal ] || { keys; return; }
  case $1 in
    h|left) [ -z "$MAX" ] && focus_dir h ;;  j|down) [ -z "$MAX" ] && focus_dir j ;;
    k|up)   [ -z "$MAX" ] && focus_dir k ;;  l|right) [ -z "$MAX" ] && focus_dir l ;;
    w|ctrlw) key_normal tab; return ;;
    +) [ -z "$MAX" ] && resize_split h 5 ;;  -) [ -z "$MAX" ] && resize_split h -5 ;;
    '>') [ -z "$MAX" ] && resize_split v 5 ;; '<') [ -z "$MAX" ] && resize_split v -5 ;;
    =) N_ratio=("${N_default[@]}"); relayout; redraw_all ;;
    o) if [ -n "$MAX" ]; then MAX=''; else MAX=$FNAME; fi; relayout; redraw_all ;;
  esac
  keys
}

# ── keys ─────────────────────────────────────────────────────────────────
start_phase() {
  nowv
  [ "$PHASE" = none ] && { note "${D}No phase is due."; return; }
  [ -n "$MAX" ] && { MAX=''; relayout; redraw_all; }
  MODE=term; TERM_I=0; TERM_WAIT=$(( NOW + 500 )); TERM_LINES=(); TERM_BACK=$FNAME
  TERM_ON=1; relayout; FNAME=claude
  note "${V}claude${R} opened for the $PHASE ${D}· it says Ready for Exit. when done"
  draw_borders; draw_claude; draw_pane "$TERM_BACK"; keys
}
open_answer() {
  inbox_items; (( ${#INBOX[@]} )) || return
  local i=${INBOX[QSEL]}
  case ${I_kind[i]} in
    phase) start_phase ;;
    q) AIDX=$i; ABUF=${I_rec[i]}; MODE=answer; DMODE=answer; local old=$FNAME; FNAME=detail; draw_pane "$old"; draw_detail; keys ;;
  esac
}
free_problem() {
  problem_items; (( ${#PROBS[@]} )) || return
  local i=${PROBS[PSEL]}
  I_alive[i]=0; T_state[${I_id[i]}]=w
  note "${OK}✓${R} #${I_id[i]} freed ${D}· the next tick starts it again"
  draw_problems; draw_tabs; draw_epics; draw_detail
}
cycle() {  # Tab: the next shown pane
  local k n=${#VISIBLE[@]} p order=()
  for p in "${ORDER_PANES[@]}"; do shown "$p" && order+=("$p"); done
  (( ${#order[@]} > 1 )) || return
  for ((k = 0; k < ${#order[@]}; k++)); do [ "${order[k]}" = "$FNAME" ] && break; done
  set_focus "${order[(k + 1) % ${#order[@]}]}"
}
key_normal() {
  case $1 in
    q) exit 0 ;;
    tab|$'\t') cycle ;;
    ctrlw) WPREFIX=1; keys ;;
    j|down)
      case $FNAME in
        inbox) QSEL=$((QSEL + 1)); DMODE=item; draw_inbox; draw_detail ;;
        problems) PSEL=$((PSEL + 1)); draw_problems; draw_detail ;;
        now) NSEL=$((NSEL + 1)); draw_now ;;
        epics) ESEL=$(( ESEL < 4 ? ESEL + 1 : 4 )); TSEL=0; [ "$DMODE" = tree ] || DMODE=epic; draw_epics; draw_detail ;;
        detail) DSCROLL=$((DSCROLL + 1)); draw_detail ;;
      esac ;;
    k|up)
      case $FNAME in
        inbox) QSEL=$(( QSEL > 0 ? QSEL - 1 : 0 )); DMODE=item; draw_inbox; draw_detail ;;
        problems) PSEL=$(( PSEL > 0 ? PSEL - 1 : 0 )); draw_problems; draw_detail ;;
        now) NSEL=$(( NSEL > 0 ? NSEL - 1 : 0 )); draw_now ;;
        epics) ESEL=$(( ESEL > 0 ? ESEL - 1 : 0 )); TSEL=0; [ "$DMODE" = tree ] || DMODE=epic; draw_epics; draw_detail ;;
        detail) DSCROLL=$(( DSCROLL > 0 ? DSCROLL - 1 : 0 )); draw_detail ;;
      esac ;;
    right) [ "$FNAME" = epics ] && { local ids=(${E_ids[ESEL]}); TSEL=$(( TSEL + 1 < ${#ids[@]} ? TSEL + 1 : ${#ids[@]} - 1 )); draw_epics; } ;;
    left)  [ "$FNAME" = epics ] && { TSEL=$(( TSEL > 0 ? TSEL - 1 : 0 )); draw_epics; } ;;
    enter)
      case $FNAME in
        inbox) open_answer ;; problems) free_problem ;;
        now) running_list; (( ${#RUNNING[@]} )) && { LIVE_T=${RUNNING[NSEL]}; draw_live; } ;;
        epics) DMODE=read; DSCROLL=0; set_focus detail ;;   # the reading takes the focus, so j/k scroll it
      esac ;;
    esc) [ "$FNAME" = detail ] && { DSCROLL=0
           case $DMODE in item) set_focus inbox ;; prob) set_focus problems ;; *) DMODE=epic; set_focus epics ;; esac; } ;;
    a) QSEL=0; [ "$FNAME" = inbox ] || set_focus inbox; open_answer ;;
    o) start_phase ;;
    s) if (( E_signed[ESEL] )); then note "${D}epic #${E_num[ESEL]} is already signed"; else pend sign "$ESEL"; draw_epics; fi ;;
    d) if [ "$DMODE" = tree ]; then DMODE=epic; else DMODE=tree; fi; DSCROLL=0; draw_detail ;;
    l) case $FNAME in
         now) running_list; (( ${#RUNNING[@]} )) && LIVE_T=${RUNNING[NSEL]} ;;
         *) LIVE_T=$(sel_ticket) ;;
       esac
       [ "${T_state[$LIVE_T]}" = r ] || note "${D}#$LIVE_T is not running: it is $(word "$LIVE_T"). w says why."
       draw_live ;;
    w) case $FNAME in now) running_list; WHY_T=${RUNNING[NSEL]:-$LIVE_T} ;; *) WHY_T=$(sel_ticket) ;; esac
       DMODE=why; DSCROLL=0; draw_detail ;;
    u) undo ;;
    :) MODE=palette; PAL_SEL=0; keys; draw_palette ;;
    1|2|3) PROJ=$(( $1 - 1 )); redraw_all ;;
  esac
}
key_answer() {
  case $1 in
    enter) I_alive[AIDX]=0; MODE=normal; FNAME=inbox; DMODE=item; pend rule "$AIDX"; draw_inbox; draw_detail; draw_tabs; keys ;;
    esc)   MODE=normal; FNAME=inbox; DMODE=item; note "${D}not ruled"; draw_inbox; draw_detail; keys ;;
    bs)    ABUF=${ABUF%?}; draw_detail ;;
    ctrlu) ABUF=''; draw_detail ;;
    up|down|left|right|other|enter|esc|tab|ctrlw) ;;
    ?*)    ABUF+=$1; draw_detail ;;
  esac
}
key_palette() {
  case $1 in
    j|down) PAL_SEL=$(( (PAL_SEL + 1) % ${#PAL[@]} )); draw_palette ;;
    k|up)   PAL_SEL=$(( (PAL_SEL + ${#PAL[@]} - 1) % ${#PAL[@]} )); draw_palette ;;
    esc)    MODE=normal; redraw_all ;;
    enter)  MODE=normal; redraw_all
            local k=${PAL[PAL_SEL]#*|}
            if [ "$k" = '^w o' ]; then window_key o; else key_normal "$k"; fi ;;
  esac
}
key_term() {  # Esc before Ready for Exit. leaves Claude with nothing to sign
  [ "$1" = esc ] || return
  local -n script="TERM_SCRIPT_$PHASE"
  if (( TERM_I >= ${#script[@]} )); then term_close; else term_close early; fi
}
term_step() {  # one line of the pretend session every few frames
  local -n script="TERM_SCRIPT_$PHASE"
  nowv; (( NOW < TERM_WAIT )) && return
  TERM_WAIT=$(( NOW + 700 ))
  if (( TERM_I < ${#script[@]} )); then
    local line=${script[TERM_I]}
    TERM_LINES+=("$line"); TERM_I=$((TERM_I + 1))
    [[ $line == *'Ready for Exit.'* ]] && { STATUS="${OK}Ready for Exit.${R} ${D}claude is done with the $PHASE"; STATUS_UNTIL=$((SECONDS + 99)); draw_status; }
    draw_claude
  elif (( TERM_I == ${#script[@]} )); then
    TERM_I=$((TERM_I + 1))     # one more beat on Ready for Exit., then close
  else
    term_close
  fi
}
term_close() {  # term_close [early]: the split closes; only its area and the borders around it are drawn again
  TERM_ON=0; MODE=signdoc; relayout; FNAME=${TERM_BACK:-inbox}; shown "$FNAME" || FNAME=inbox
  draw_borders; draw_detail; draw_live
  if [ "${1:-}" = early ]; then
    MODE=normal; note "${D}Left claude early: $PHASE.md is not written. o reopens it"
    draw_pane "$FNAME"; keys; return
  fi
  STATUS="${B}Sign $PHASE.md?${R} It says what you mean, and an edit later unsigns it. [${OK}Y${R}/n]"; STATUS_UNTIL=$((SECONDS + 99))
  draw_status; keys
}
key_signdoc() {
  case $1 in
    enter|y|Y) local doc=$PHASE; MODE=normal
               if [ "$PHASE" = objectives ]; then PHASE=design; else PHASE=none; I_alive[0]=0; fi
               pend doc "$doc"; draw_inbox; draw_detail; draw_tabs; draw_pane "$FNAME" ;;
    n|N|esc) MODE=normal; note "${D}$PHASE.md is not signed; o opens it again"; keys; draw_pane "$FNAME" ;;
    q) exit 0 ;;   # quitting leaves it unsigned; o opens it again
  esac
}

read_key() {  # read_key <timeout>: one key, or nothing; arrows and Esc decoded
  local k rest
  IFS= read -rsn1 -t "$1" k || return 1
  case $k in
    $'\e') IFS= read -rsn2 -t 0.02 rest
           case $rest in '[A'|OA) KEY=up ;; '[B'|OB) KEY=down ;; '[C'|OC) KEY=right ;; '[D'|OD) KEY=left ;; '[Z') KEY=other ;; '') KEY=esc ;; *) KEY=other ;; esac ;;
    ''|$'\r'|$'\n') KEY=enter ;;
    $'\t') KEY=tab ;;
    $'\x7f'|$'\b') KEY=bs ;;
    $'\x15') KEY=ctrlu ;;
    $'\x17') KEY=ctrlw ;;
    *) KEY=$k ;;
  esac
}

handle_key() {
  if (( WPREFIX )); then window_key "$KEY"; return; fi
  case $MODE in
    normal) key_normal "$KEY" ;; answer) key_answer "$KEY" ;; palette) key_palette "$KEY" ;;
    term) key_term "$KEY" ;; signdoc) key_signdoc "$KEY" ;;
  esac
}

# ── run ──────────────────────────────────────────────────────────────────
nowv; NEXT_TICK=$(( NOW + 1500 ))
cleanup() {
  printf '%s' "$E[0m$E[?25h$E[?1049l"
  stty "$SAVED_STTY" 2>/dev/null
  printf '%szaf%s closed. The build keeps running; %szaf%s opens this again.\n' "$Z" "$R" "$Z" "$R"
}
SAVED_STTY=$(stty -g)
trap cleanup EXIT
trap 'exit 130' INT TERM
trap 'RESIZED=1' WINCH
printf '%s' "$E[?1049h$E[?25l"
stty -echo -icanon -iexten -ixon min 1 time 0   # keys reach the mock as typed: Ctrl-w and Ctrl-u are not line editing

while :; do
  if (( RESIZED )); then RESIZED=0; screen_size; relayout; OUT+="$E[2J"; redraw_all; fi
  if (( COLS >= 80 && ROWS >= 24 )); then
    if read_key 0.1; then
      handle_key; while read_key 0.002; do handle_key; done
    fi
    FRAME=$((FRAME + 1))
    [ "$MODE" = term ] && term_step
    nowv; (( NOW >= NEXT_TICK )) && { NEXT_TICK=$(( NOW + 1500 )); tick; }
    if (( FRAME % 2 == 0 )) && [ "$MODE" != palette ]; then   # the spinners, where they show
      draw_tabs; draw_epics; draw_now; draw_live
    fi
    pend_check
    if [ -n "$STATUS" ] && [ -z "$PEND" ] && [ "$MODE" = normal ] && (( SECONDS >= STATUS_UNTIL )); then STATUS=''; draw_status; fi
    flush
  else
    read_key 0.1; [ "${KEY:-}" = q ] && exit 0
  fi
done
