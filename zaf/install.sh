#!/bin/sh
# zaf's install script: curl -fsSL <release>/install.sh | sh
#
# It shows what it will do and asks before it does anything. Every line below is
# inside a function and the last line is the one call, so a download cut short
# runs nothing (the way rustup-init.sh does it).
#
#   install.sh [--dir <folder>] [--yes]
#
# Questions are read from /dev/tty, since `curl ... | sh` leaves the script no
# stdin of its own. With no terminal at all it refuses unless --yes, which takes
# every default: curl -fsSL <release>/install.sh | sh -s -- --yes
#
# ZAF_INSTALL_TTY names a file to read the answers from instead of /dev/tty; it
# is there for the tests.
#
# The release workflows rewrite the next line, and only that line.
ZAF_VERSION="0.0.0-dev"

init() {
  MIN_CLAUDE=2.1.296
  REPO_URL=https://github.com/ProgrammingCheetah/ways
  ASSUME_YES=0
  DIR_GIVEN=0
  DIR=
  DIR_SHOWN=
  WORK=
  STEP_N=0
  STEP_DONE=
  KEEP_LOG=0
  SPIN_PID=
  TARGET=
  SUM_CMD=
  SHELL_FILE=
  SHELL_FILE_SHOWN=
  ZC=
}

usage() {
  printf 'usage: install.sh [--dir <folder>] [--yes]\n'
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case $1 in
      --yes | -y) ASSUME_YES=1 ;;
      --dir)
        if [ "$#" -lt 2 ] || [ -z "$2" ]; then
          printf 'zaf install: --dir needs a folder.\n' >&2
          usage >&2
          return 1
        fi
        DIR=$2
        DIR_GIVEN=1
        shift
        ;;
      --dir=*)
        DIR=${1#--dir=}
        if [ -z "$DIR" ]; then
          printf 'zaf install: --dir needs a folder.\n' >&2
          usage >&2
          return 1
        fi
        DIR_GIVEN=1
        ;;
      --help | -h)
        usage
        exit 0
        ;;
      *)
        printf 'zaf install: unknown option %s\n' "$1" >&2
        usage >&2
        return 1
        ;;
    esac
    shift
  done
  return 0
}

# Colour as the install mock; none when NO_COLOR is set and not empty, or when
# stdout is not a terminal.
colours() {
  IS_TERM=0
  if [ -t 1 ]; then IS_TERM=1; fi
  if [ "$IS_TERM" = 1 ] && [ -z "${NO_COLOR:-}" ]; then
    B=$(printf '\033[1m')
    D=$(printf '\033[2m')
    R=$(printf '\033[0m')
    V=$(printf '\033[1;36m')
    N=$(printf '\033[33m')
    G=$(printf '\033[32m')
    Z=$(printf '\033[1;38;5;201m')
    X=$(printf '\033[31m')
  else
    B= D= R= V= N= G= Z= X=
  fi
}

# zc <text> <colour to go back to>: the text with every "zaf" in its own colour,
# left in ZC.
zc() {
  zc_rest=$1
  zc_out=
  while :; do
    case $zc_rest in
      *zaf*)
        zc_out=$zc_out${zc_rest%%zaf*}${Z}zaf${R}$2
        zc_rest=${zc_rest#*zaf}
        ;;
      *)
        zc_out=$zc_out$zc_rest
        break
        ;;
    esac
  done
  ZC=$zc_out
}

# Paths: a typed ~ is expanded; the screen shows $HOME as ~.
expand_tilde() {
  case $1 in
    "~") printf '%s' "$HOME" ;;
    "~/"*) printf '%s/%s' "$HOME" "${1#"~/"}" ;;
    *) printf '%s' "$1" ;;
  esac
}

show_tilde() {
  case $1 in
    "$HOME") printf '~' ;;
    "$HOME"/*) printf '~/%s' "${1#"$HOME"/}" ;;
    *) printf '%s' "$1" ;;
  esac
}

# set_dir <folder>: takes the folder when, once ~ is expanded, it starts with /. Anything else
# (bin, .local/bin, ~user/x, which is not expanded) is refused, naming ~/.local/bin, and DIR keeps
# its value: a relative folder would put a relative entry in the shell's rc file. A folder holding
# `:` is refused too: it would split into two entries in PATH.
set_dir() {
  sd_dir=$(expand_tilde "$1")
  case $sd_dir in
    /*) ;;
    *)
      printf '%s is not a full folder: it must start with / or ~/, such as ~/.local/bin.\n' "$1" >&2
      return 1
      ;;
  esac
  case $sd_dir in
    *:*)
      printf 'a folder in PATH cannot hold `:`\n' >&2
      return 1
      ;;
  esac
  DIR=$sd_dir
  case $DIR in
    / | "") ;;
    */) DIR=${DIR%/} ;;
  esac
  DIR_SHOWN=$(show_tilde "$DIR")
}

# The terminal the questions are read from, held open on fd 3.
open_terminal() {
  if [ "$ASSUME_YES" = 1 ]; then
    return 0
  fi
  TTY_FILE=${ZAF_INSTALL_TTY:-/dev/tty}
  if ( exec 3<"$TTY_FILE" ) 2>/dev/null; then
    exec 3<"$TTY_FILE"
    return 0
  fi
  printf 'zaf install: there is no terminal to ask on, so nothing was changed.\n' >&2
  printf 'Run it from a terminal, or take every default with --yes:\n' >&2
  printf '  curl -fsSL %s/releases/latest/download/install.sh | sh -s -- --yes\n' "$REPO_URL" >&2
  return 1
}

# ask: one line from the terminal into ANSWER; fails at the end of the input.
ask() {
  ANSWER=
  IFS= read -r ANSWER <&3
}

# The file PATH goes in, from $SHELL's name (I6). Left in SHELL_FILE.
shell_file() {
  sf_shell=${SHELL:-}
  sf_shell=${sf_shell##*/}
  case $sf_shell in
    zsh) SHELL_FILE=${ZDOTDIR:-$HOME}/.zshrc ;;
    bash)
      if [ "$(uname -s 2>/dev/null)" = Darwin ]; then
        SHELL_FILE=$HOME/.bash_profile
        for sf_f in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
          if [ -e "$sf_f" ]; then
            SHELL_FILE=$sf_f
            break
          fi
        done
      else
        SHELL_FILE=$HOME/.bashrc
      fi
      ;;
    fish) SHELL_FILE=$HOME/.config/fish/conf.d/zaf.fish ;;
    *) SHELL_FILE=$HOME/.profile ;;
  esac
  SHELL_FILE_SHOWN=$(show_tilde "$SHELL_FILE")
}

# row <n> <verb> <what> <why>
row() {
  row_noun=$(printf '%-34s' "$3")
  zc "$row_noun" "$N"
  row_noun=$ZC
  zc "$4" "$D"
  row_why=$ZC
  printf '  %s%s.%s %s%-8s%s%s%s%s %s%s%s\n' "$D" "$1" "$R" "$V" "$2" "$R" "$N" "$row_noun" "$R" "$D" "$row_why" "$R"
}

show_intro() {
  printf '%szaf%s: %sZ%suri'"'"'s %sA%smazing %sF%sramework\n' "$Z" "$R" "$Z" "$R" "$Z" "$R" "$Z" "$R"
  printf 'It runs a software project'"'"'s workflow %s(plan, build, review, release)%s\n' "$D" "$R"
  printf 'by driving your AI agents for you. %sToday that agent is Claude Code.%s\n\n' "$D" "$R"
}

folder_prompt() {
  printf '%sWhere should %szaf%s%s be installed?%s [%s%s%s] ' "$B" "$Z" "$R" "$B" "$R" "$N" "$DIR_SHOWN" "$R"
}

# Asks again after a refused answer; Enter, or the end of the input, keeps the default.
ask_folder() {
  folder_prompt
  if [ "$DIR_GIVEN" = 1 ] || [ "$ASSUME_YES" = 1 ]; then
    printf '\n\n'
    return 0
  fi
  while ask && [ -n "$ANSWER" ]; do
    if set_dir "$ANSWER"; then
      break
    fi
    folder_prompt
  done
  printf '\n'
}

show_plan() {
  printf 'This script will do %s5 things%s:\n\n' "$B" "$R"
  row 1 Check 'git, Claude Code 2.1.296+' 'zaf drives both; nothing is installed for you'
  row 2 Install "zaf $ZAF_VERSION in $DIR_SHOWN" 'the program you will type commands to'
  row 3 Add "$DIR_SHOWN to PATH ($SHELL_FILE_SHOWN)" 'so zaf works in any terminal'
  row 4 Add 'the teams plugin to Claude Code' 'the roles zaf starts inside Claude Code'
  row 5 Show 'your first three commands' 'so you know what to type next'
  printf '\nIt changes %snothing else%s, and it does not touch any project.\n\n' "$B" "$R"
}

# confirm: 0 for yes, 1 for no. Anything but n or N is yes, and so is --yes; the
# end of the input is no.
confirm() {
  printf '%sContinue?%s [%sY%s/n] ' "$B" "$R" "$G" "$R"
  if [ "$ASSUME_YES" = 1 ]; then
    printf 'Y\n\n'
    return 0
  fi
  if ! ask && [ -z "$ANSWER" ]; then
    printf '\n'
    return 1
  fi
  printf '\n'
  case $ANSWER in
    [nN]*) return 1 ;;
  esac
  return 0
}

# The step-line drawer. One line per step: a spinner redrawn in place in a
# terminal, then a check mark or a cross. A tool's output goes to a log in a
# temporary folder; on a cross, its last five lines and the log's path, and the
# log is kept.

make_work() {
  if [ -n "$WORK" ]; then
    return 0
  fi
  WORK=$(mktemp -d "${TMPDIR:-/tmp}/zaf-install.XXXXXX") || {
    printf 'zaf install: cannot make a temporary folder, so nothing was changed.\n' >&2
    return 1
  }
}

spinner_start() {
  (
    set -- '⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏'
    while :; do
      printf '\r\033[K  %s%s%s %s%-8s%s%s%s%s' "$V" "$1" "$R" "$V" "$SP_VERB" "$R" "$N" "$SP_NOUN" "$R"
      sp_first=$1
      shift
      set -- "$@" "$sp_first"
      sleep 0.08
    done
  ) &
  SPIN_PID=$!
}

spinner_stop() {
  if [ -n "$SPIN_PID" ]; then
    kill "$SPIN_PID" 2>/dev/null
    wait "$SPIN_PID" 2>/dev/null
    SPIN_PID=
    printf '\r\033[K'
  fi
}

# step_run <verb> <what> <done text> <command> [args]: run the command with its
# output in the log. The done text may be empty when the command leaves its own
# in STEP_DONE.
step_run() {
  SP_VERB=$1
  SP_NOUN=$2
  sr_done=$3
  shift 3
  STEP_DONE=
  make_work || return 1
  STEP_N=$((STEP_N + 1))
  sr_log=$WORK/step-$STEP_N.log
  : >"$sr_log"
  if [ "$IS_TERM" = 1 ]; then
    spinner_start
  fi
  "$@" >>"$sr_log" 2>&1
  sr_rc=$?
  spinner_stop
  if [ "$sr_rc" -eq 0 ]; then
    zc "${sr_done:-$STEP_DONE}" "$N"
    printf '  %s✓%s %s%-8s%s%s%s%s\n' "$G" "$R" "$V" "$SP_VERB" "$R" "$N" "$ZC" "$R"
    return 0
  fi
  KEEP_LOG=1
  zc "$SP_NOUN" "$N"
  printf '  %s✗%s %s%-8s%s%s%s%s\n' "$X" "$R" "$V" "$SP_VERB" "$R" "$N" "$ZC" "$R"
  tail -n 5 "$sr_log" | while IFS= read -r sr_line; do
    printf '    %s\n' "$sr_line"
  done >&2
  printf '    The log is kept: %s\n' "$sr_log" >&2
  return 1
}

cleanup() {
  spinner_stop
  if [ -n "$WORK" ] && [ "$KEEP_LOG" != 1 ]; then
    rm -rf "$WORK"
  fi
}

# Step 1. Each refusal says what failed and the fix, in at most five lines, so
# the drawer shows all of it.

check_platform() {
  cp_os=$(uname -s 2>/dev/null)
  cp_arch=$(uname -m 2>/dev/null)
  TARGET=
  case "$cp_os $cp_arch" in
    "Linux x86_64") TARGET=x86_64-unknown-linux-musl ;;
    "Darwin arm64" | "Darwin aarch64") TARGET=aarch64-apple-darwin ;;
    "Darwin x86_64")
      # Under Rosetta, uname says x86_64 on Apple silicon.
      if [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ]; then
        TARGET=aarch64-apple-darwin
      fi
      ;;
  esac
  if [ -z "$TARGET" ]; then
    printf 'zaf is released for Linux x86_64 and macOS Apple silicon.\n'
    printf 'On %s %s, build it:\n' "$cp_os" "$cp_arch"
    printf '  cargo install --git %s --tag zaf-v%s --locked zaf\n' "$REPO_URL" "$ZAF_VERSION"
    return 1
  fi
  return 0
}

check_tool() {
  if command -v "$1" >/dev/null 2>&1; then
    return 0
  fi
  printf '%s is not installed.\n' "$1"
  printf 'Install %s (%s), then run this script again.\n' "$1" "$2"
  return 1
}

check_sum_tool() {
  if command -v sha256sum >/dev/null 2>&1; then
    SUM_CMD=sha256sum
    return 0
  fi
  if command -v shasum >/dev/null 2>&1; then
    SUM_CMD="shasum -a 256"
    return 0
  fi
  printf 'Neither sha256sum nor shasum is installed.\n'
  printf 'Install sha256sum (coreutils) or shasum (perl), then run this script again.\n'
  return 1
}

# version_at_least <have> <want>: 0 when have is want or later.
version_at_least() {
  va_old=$IFS
  IFS=.
  set -- $1 $2
  IFS=$va_old
  va_a1=${1:-0} va_a2=${2:-0} va_a3=${3:-0}
  va_b1=${4:-0} va_b2=${5:-0} va_b3=${6:-0}
  # A suffix such as -beta is not part of the number.
  va_a1=${va_a1%%[!0-9]*} va_a2=${va_a2%%[!0-9]*} va_a3=${va_a3%%[!0-9]*}
  va_b1=${va_b1%%[!0-9]*} va_b2=${va_b2%%[!0-9]*} va_b3=${va_b3%%[!0-9]*}
  va_a1=${va_a1:-0} va_a2=${va_a2:-0} va_a3=${va_a3:-0}
  va_b1=${va_b1:-0} va_b2=${va_b2:-0} va_b3=${va_b3:-0}
  [ "$va_a1" -gt "$va_b1" ] && return 0
  [ "$va_a1" -lt "$va_b1" ] && return 1
  [ "$va_a2" -gt "$va_b2" ] && return 0
  [ "$va_a2" -lt "$va_b2" ] && return 1
  [ "$va_a3" -ge "$va_b3" ]
}

check_claude() {
  check_tool claude 'Claude Code: https://claude.com/product/claude-code' || return 1
  cc_out=$(claude --version 2>&1)
  CLAUDE_VERSION=${cc_out%% *}
  case $CLAUDE_VERSION in
    [0-9]*) ;;
    *)
      printf 'Could not read the version `claude --version` printed: %s\n' "$cc_out"
      printf 'Claude Code %s or later is needed. Fix claude, then run this script again.\n' "$MIN_CLAUDE"
      return 1
      ;;
  esac
  if ! version_at_least "$CLAUDE_VERSION" "$MIN_CLAUDE"; then
    printf 'Claude Code %s is older than %s, the minimum zaf needs.\n' "$CLAUDE_VERSION" "$MIN_CLAUDE"
    printf 'Update it (`claude update`), then run this script again.\n'
    return 1
  fi
  return 0
}

# A folder that exists and cannot be written stops the script; it never uses sudo.
check_folder() {
  if [ -e "$DIR" ] && { [ ! -d "$DIR" ] || [ ! -w "$DIR" ]; }; then
    printf '%s exists and cannot be written, and this script never uses sudo.\n' "$DIR_SHOWN"
    printf 'Pick a folder you own, such as ~/.local/bin: run it again with --dir ~/.local/bin.\n'
    return 1
  fi
  return 0
}

check_all() {
  check_platform || return 1
  check_tool git 'https://git-scm.com/downloads' || return 1
  check_tool curl 'your package manager has it' || return 1
  check_tool tar 'your package manager has it' || return 1
  check_sum_tool || return 1
  check_claude || return 1
  check_folder || return 1
  ca_git=$(git --version 2>/dev/null)
  ca_git=${ca_git##* }
  STEP_DONE="git $ca_git, Claude Code $CLAUDE_VERSION, $TARGET"
  return 0
}

step_check() {
  step_run Check 'git and Claude Code' '' check_all
}

# Steps 2 and 3. The archive and its sum come from this release on GitHub, or
# from ZAF_INSTALL_FROM (a folder or an https URL; it changes only where the files come
# from, the sum is checked all the same). Everything is done in the temporary
# folder first: the chosen folder is made and written only once the unpacked zaf
# has printed its version, so a failure changes nothing in it.

# fetch <name>: the file from the release into the temporary folder.
fetch() {
  case ${ZAF_INSTALL_FROM:-} in
    "")
      curl -fsSL --proto '=https' --tlsv1.2 -o "$WORK/$1" "$REPO_URL/releases/download/zaf-v$ZAF_VERSION/$1"
      ;;
    *://*)
      curl -fsSL --proto '=https' --tlsv1.2 -o "$WORK/$1" "${ZAF_INSTALL_FROM%/}/$1"
      ;;
    *)
      cp "${ZAF_INSTALL_FROM%/}/$1" "$WORK/$1"
      ;;
  esac
}

# The archive's sum against the first word of its .sha256 file.
verify_archive() {
  va_file=$WORK/$1
  va_want=$(head -n 1 "$va_file.sha256" | tr 'A-F' 'a-f')
  va_want=${va_want%%[ 	]*}
  case $va_want in
    *[!0-9a-f]* | "")
      printf '%s.sha256 does not hold a sum.\n' "$1"
      return 1
      ;;
  esac
  va_got=$($SUM_CMD "$va_file")
  va_got=${va_got%%[ 	]*}
  if [ "$va_got" != "$va_want" ]; then
    printf 'The sha256 of %s is %s,\n' "$1" "$va_got"
    printf 'but the release says %s. Nothing was installed.\n' "$va_want"
    return 1
  fi
  return 0
}

install_zaf() {
  # A URL source takes only https, like the real download: refused before anything is fetched.
  case ${ZAF_INSTALL_FROM:-} in
    https://*) ;;
    *://*)
      printf 'ZAF_INSTALL_FROM takes a folder or an https URL, not %s.\n' "$ZAF_INSTALL_FROM"
      return 1
      ;;
  esac
  iz_name=zaf-v$ZAF_VERSION-$TARGET
  printf 'Downloading %s.tar.gz from %s\n' "$iz_name" "${ZAF_INSTALL_FROM:-$REPO_URL/releases/download/zaf-v$ZAF_VERSION}"
  fetch "$iz_name.tar.gz" || return 1
  fetch "$iz_name.tar.gz.sha256" || return 1
  verify_archive "$iz_name.tar.gz" || return 1
  mkdir -p "$WORK/unpacked" || return 1
  tar -xzf "$WORK/$iz_name.tar.gz" -C "$WORK/unpacked" || {
    printf 'tar could not unpack %s.tar.gz.\n' "$iz_name"
    return 1
  }
  iz_fresh=$WORK/unpacked/$iz_name/zaf
  if [ ! -f "$iz_fresh" ]; then
    printf 'The archive holds no %s/zaf.\n' "$iz_name"
    return 1
  fi
  chmod 755 "$iz_fresh" || return 1
  iz_said=$("$iz_fresh" --version 2>&1)
  if [ "$iz_said" != "zaf $ZAF_VERSION" ]; then
    printf 'The new binary says `%s`, not `zaf %s`. Nothing was installed.\n' "$iz_said" "$ZAF_VERSION"
    return 1
  fi
  # Written beside the old zaf and renamed over it, so a running zaf is never
  # written into (as `zaf update` does it).
  mkdir -p "$DIR" || return 1
  if [ -d "$DIR/zaf" ]; then
    printf '%s/zaf is a folder, so zaf cannot be put there.\n' "$DIR_SHOWN"
    return 1
  fi
  iz_beside=$DIR/.zaf-install.$$
  if ! cp "$iz_fresh" "$iz_beside" || ! chmod 755 "$iz_beside" || ! mv -f "$iz_beside" "$DIR/zaf"; then
    rm -f "$iz_beside"
    printf 'Could not put zaf in %s.\n' "$DIR_SHOWN"
    return 1
  fi
  STEP_DONE="zaf $ZAF_VERSION in $DIR_SHOWN"
  return 0
}

step_install() {
  step_run Install "zaf $ZAF_VERSION (download, sum, unpack)" '' install_zaf
}

# Step 4. The folder goes on PATH in the shell's file (I6), in one block marked
# PATH_MARK. Nothing is written when the folder is already on PATH or the block
# already names it; a block that names another folder gets the new one in its place.
PATH_MARK="# added by zaf's install script"

# on_path <folder>: 0 when it is one of PATH's entries.
on_path() {
  op_rest=${PATH:-}:
  while [ -n "$op_rest" ]; do
    op_entry=${op_rest%%:*}
    op_rest=${op_rest#*:}
    op_entry=${op_entry%/}
    if [ "$op_entry" = "$1" ]; then
      return 0
    fi
  done
  return 1
}

# path_quote <text> <shell>: the folder escaped to sit inside double quotes, $HOME
# left to the shell so the file still works if the home moves. Left in PQ.
path_quote() {
  pq_text=$1
  pq_head=
  case $pq_text in
    "$HOME"/*)
      pq_head='$HOME'
      pq_text=/${pq_text#"$HOME"/}
      ;;
  esac
  pq_out=
  while [ -n "$pq_text" ]; do
    pq_c=${pq_text%"${pq_text#?}"}
    pq_text=${pq_text#?}
    case $pq_c in
      '\' | '"' | '$') pq_out=$pq_out\\$pq_c ;;
      '`')
        if [ "$2" = fish ]; then pq_out=$pq_out$pq_c; else pq_out=$pq_out\\$pq_c; fi
        ;;
      *) pq_out=$pq_out$pq_c ;;
    esac
  done
  PQ=$pq_head$pq_out
}

# path_line <shell>: the line the block holds for DIR, left in PL.
path_line() {
  path_quote "$DIR" "$1"
  if [ "$1" = fish ]; then
    PL="fish_add_path -- \"$PQ\""
  else
    PL="export PATH=\"$PQ:\$PATH\""
  fi
}

# path_old_folder <line>: the folder a block's line names, as the screen shows it. A line
# this script did not write is shown as it is. Left in POLD.
path_old_folder() {
  po_text=$1
  case $po_text in
    'export PATH="'*':$PATH"')
      po_text=${po_text#'export PATH="'}
      po_text=${po_text%':$PATH"'}
      ;;
    'fish_add_path -- "'*'"')
      po_text=${po_text#'fish_add_path -- "'}
      po_text=${po_text%'"'}
      ;;
    *)
      POLD=$po_text
      return 0
      ;;
  esac
  po_head=
  case $po_text in
    '$HOME'/*)
      po_head=$HOME
      po_text=${po_text#'$HOME'}
      ;;
  esac
  po_out=
  while [ -n "$po_text" ]; do
    po_c=${po_text%"${po_text#?}"}
    po_text=${po_text#?}
    if [ "$po_c" = '\' ] && [ -n "$po_text" ]; then
      po_c=${po_text%"${po_text#?}"}
      po_text=${po_text#?}
    fi
    po_out=$po_out$po_c
  done
  POLD=$(show_tilde "$po_head$po_out")
}

# path_replace: the block already in SHELL_FILE gets the line for DIR, the file's other
# lines as they were, and the screen names the folder it replaced. A block that already
# names DIR leaves the file alone.
path_replace() {
  pr_tmp=$(mktemp "${TMPDIR:-/tmp}/zaf-rc.XXXXXX") || return 1
  pr_old=
  pr_seen=0
  pr_next=0
  pr_nl=
  {
    while :; do
      pr_eof=0
      IFS= read -r pr_line || pr_eof=1
      if [ "$pr_eof" = 1 ] && [ -z "$pr_line" ]; then
        break
      fi
      if [ "$pr_eof" = 1 ]; then pr_nl=; else pr_nl=y; fi
      if [ "$pr_next" = 1 ]; then
        if [ "$pr_seen" = 0 ]; then
          pr_old=$pr_line
          pr_seen=1
        fi
        pr_next=0
        printf '%s' "$PL"
      else
        printf '%s' "$pr_line"
        if [ "$pr_line" = "$PATH_MARK" ]; then pr_next=1; fi
      fi
      if [ -n "$pr_nl" ]; then printf '\n'; fi
      if [ "$pr_eof" = 1 ]; then break; fi
    done
    # A mark that ends the file has no line to replace: the block gets its line.
    if [ "$pr_next" = 1 ]; then
      if [ -z "$pr_nl" ]; then printf '\n'; fi
      printf '%s\n' "$PL"
    fi
  } <"$SHELL_FILE" >"$pr_tmp"
  if [ "$pr_old" = "$PL" ]; then
    rm -f "$pr_tmp"
    STEP_DONE="$SHELL_FILE_SHOWN already has the block, nothing written"
    return 0
  fi
  path_old_folder "$pr_old"
  # The rewrite truncates the file first, so a copy of it is kept until the rewrite is
  # done. A copy left by an earlier failed run is the only full one: never written over.
  pr_bak=$SHELL_FILE.zaf-backup
  if [ -e "$pr_bak" ]; then
    printf '%s is already there, from an earlier run. Check it, move it away, and run again.\n' "$pr_bak"
    rm -f "$pr_tmp"
    return 1
  fi
  if ! cp "$SHELL_FILE" "$pr_bak"; then
    rm -f "$pr_bak" "$pr_tmp"
    printf 'Could not back up %s, so it was not changed.\n' "$SHELL_FILE_SHOWN"
    return 1
  fi
  if ! cat "$pr_tmp" >"$SHELL_FILE"; then
    # The backup stays, and so does the temporary copy of the new file.
    printf 'Could not write %s. Its earlier contents are kept in %s (the new text is in %s).\n' \
      "$SHELL_FILE_SHOWN" "$pr_bak" "$pr_tmp"
    return 1
  fi
  rm -f "$pr_tmp" "$pr_bak"
  STEP_DONE="$DIR_SHOWN replaced $POLD in the block of $SHELL_FILE_SHOWN"
  return 0
}

path_add() {
  if on_path "$DIR"; then
    STEP_DONE="$DIR_SHOWN is already on PATH, nothing written"
    return 0
  fi
  case $SHELL_FILE in
    */fish/conf.d/zaf.fish) pa_shell=fish ;;
    *) pa_shell=sh ;;
  esac
  path_line "$pa_shell"
  if [ -f "$SHELL_FILE" ] && grep -qxF "$PATH_MARK" "$SHELL_FILE"; then
    path_replace
    return
  fi
  mkdir -p "$(dirname "$SHELL_FILE")" || return 1
  {
    # A file that does not end in a newline gets one, so the block starts a line.
    if [ -s "$SHELL_FILE" ]; then
      if [ -n "$(tail -c 1 "$SHELL_FILE")" ]; then
        printf '\n'
      fi
      printf '\n'
    fi
    printf '%s\n' "$PATH_MARK"
    printf '%s\n' "$PL"
  } >>"$SHELL_FILE" || {
    printf 'Could not write %s.\n' "$SHELL_FILE_SHOWN"
    return 1
  }
  STEP_DONE="$DIR_SHOWN added to PATH in $SHELL_FILE_SHOWN"
  return 0
}

step_path() {
  step_run Add "$DIR_SHOWN to PATH" '' path_add
}

# After the steps: when another zaf is found on PATH before the installed one,
# say which. A folder not on PATH yet comes first in a new terminal, but this one
# still finds the other.
report_shadow() {
  rs_rest=${PATH:-}:
  rs_found=
  while [ -n "$rs_rest" ]; do
    rs_entry=${rs_rest%%:*}
    rs_rest=${rs_rest#*:}
    rs_entry=${rs_entry%/}
    if [ "$rs_entry" = "$DIR" ]; then
      return 0
    fi
    if [ -n "$rs_entry" ] && [ -f "$rs_entry/zaf" ] && [ -x "$rs_entry/zaf" ]; then
      rs_found=$rs_entry/zaf
      break
    fi
  done
  if [ -z "$rs_found" ]; then
    return 0
  fi
  if on_path "$DIR"; then
    zc "Another zaf comes first on PATH: $rs_found. The one just installed is $DIR_SHOWN/zaf." "$R"
  else
    zc "This terminal finds another zaf first: $rs_found. A new terminal finds the one just installed, $DIR_SHOWN/zaf." "$R"
  fi
  printf '\n%s%s%s\n' "$N" "$ZC" "$R"
}

# Step 5. The marketplace is added when `claude plugin marketplace list` has no
# `ways` (a line that opens with the name), then the plugin is installed, or
# updated when `claude plugin list` already has it. The tools' own words go to
# the log.
PLUGIN=teams@ways
MARKET_REPO=ProgrammingCheetah/ways

plugin_add() {
  pl_markets=$(claude plugin marketplace list 2>&1) || {
    printf '%s\n' "$pl_markets"
    return 1
  }
  if ! printf '%s\n' "$pl_markets" | grep -Eq '^[^[:alnum:]]*ways([[:space:]]|$)'; then
    claude plugin marketplace add "$MARKET_REPO" || return 1
  fi
  pl_have=$(claude plugin list 2>&1) || {
    printf '%s\n' "$pl_have"
    return 1
  }
  if printf '%s\n' "$pl_have" | grep -qF "$PLUGIN"; then
    claude plugin update "$PLUGIN" || return 1
    STEP_DONE="the teams plugin updated in Claude Code ($PLUGIN)"
  else
    claude plugin install "$PLUGIN" || return 1
    STEP_DONE="the teams plugin to Claude Code ($PLUGIN)"
  fi
  return 0
}

step_plugin() {
  step_run Add 'the teams plugin to Claude Code' '' plugin_add
}

# Step 6, as the end of the install mock. Plain text is padded first and coloured
# after, so the columns line up.
# show_cmd <number> <command> <words>
show_cmd() {
  zc "$(printf '%-34s' "$2")" "$V"
  sc_cmd=$ZC
  zc "$3" "$D"
  printf '  %s%-3s%s%s%s%s %s%s%s\n' "$D" "$1" "$R" "$V" "$sc_cmd" "$R" "$D" "$ZC" "$R"
}

step_show() {
  printf '\n%szaf%s is installed. %sOpen a new terminal%s first, so PATH picks it up.\n' "$Z" "$R" "$B" "$R"
  printf '\nStart working on anything in %s3 commands%s, from your project'"'"'s folder:\n\n' "$B" "$R"
  show_cmd 1. 'zaf setup' 'connects zaf to your project: board, forge, check'
  show_cmd 2. 'zaf begin --version 0.1.0' 'you say what you want; it becomes objectives, a design, tickets'
  show_cmd 3. 'zaf spawn --watch --version 0.1.0' "zaf's agents build the tickets until they are done"
  printf '\n'
  show_cmd '' 'zaf hub' 'watch them work, live'
  show_cmd '' 'zaf status' 'where everything stands, on one screen'
  show_cmd '' 'zaf help' 'every command; add --help to any one'
  printf '\n'
  printf '%sThe teams plugin is what zaf starts inside Claude Code; you never call it yourself.%s\n\n' "$D" "$R"
}

main() {
  init
  if [ "$(id -u 2>/dev/null)" = 0 ]; then
    printf 'zaf install: this script runs as you and never uses sudo, so run it again as your own user; nothing was changed.\n' >&2
    return 1
  fi
  parse_args "$@" || return 1
  colours
  if [ -z "${HOME:-}" ]; then
    printf 'zaf install: HOME is not set, so nothing was changed.\n' >&2
    return 1
  fi
  open_terminal || return 1
  trap cleanup EXIT
  trap 'exit 1' INT TERM HUP
  if [ "$DIR_GIVEN" = 1 ]; then
    set_dir "$DIR" || return 1
  else
    set_dir "$HOME/.local/bin"
  fi
  show_intro
  ask_folder
  shell_file
  show_plan
  if ! confirm; then
    printf 'Nothing changed.\n'
    return 0
  fi
  step_check || return 1
  step_install || return 1
  step_path || return 1
  step_plugin || return 1
  step_show || return 1
  report_shadow
  return 0
}

main "$@" || exit 1
