#!/bin/sh
# Take the pictures in docs/FEATURE_GUIDE.md, from the program itself.
#
# Every picture is copal-tm drawing against its simulation (--simulate), so
# no reading, process name or path in them comes from the machine that took
# them.  Two ways in:
#
#   still  one frame from `copal-tm --frame WxH [--open ...]`, the same path
#          `make shot` and the golden-frame tests use;
#   live   the real event loop in a pseudo-terminal (script(1)), driven by
#          keystrokes, for the states --open cannot reach.  Only keys that
#          open, move and look are ever sent.  Nothing here confirms a plan
#          or a Services item: under --simulate a confirmed signal would
#          still go to kill(2) on this machine, at the simulation's pids.
#
# Either way the escape stream is replayed by tools/frame2html.rs (rustc,
# std only) into an HTML page, rings are drawn where the captions point,
# and headless Chromium photographs it.
#
# Needs: cargo and rustc (as the build does), script(1) from util-linux,
# timeout(1), and chromium.  Writes docs/features/NN-name.png; scratch in target/.
#
#   sh tools/feature-guide.sh            # every picture
#   sh tools/feature-guide.sh 05 12      # only those numbers
set -eu

cd "$(dirname "$0")/.."

BIN=$PWD/target/release/copal-tm
WORK=$PWD/target/feature-guide
OUT=$PWD/docs/features
CHROMIUM=${CHROMIUM:-chromium}
ONLY="$*"

mkdir -p "$WORK" "$OUT"
cargo build --release --locked >&2
rustc --edition 2021 -O -o "$WORK/frame2html" tools/frame2html.rs

wanted() {
	[ -z "$ONLY" ] && return 0
	for n in $ONLY; do [ "$n" = "$1" ] && return 0; done
	return 1
}

# shoot NAME COLS ROWS [ring ...] < stream
shoot() {
	name=$1 cols=$2 rows=$3
	shift 3
	"$WORK/frame2html" "$cols" "$rows" "$@" > "$WORK/$name.html"
	"$CHROMIUM" --headless=new --no-sandbox --hide-scrollbars \
		--window-size="$((cols * 10 + 28)),$((rows * 20 + 28))" \
		--screenshot="$OUT/$name.png" "file://$WORK/$name.html" 2>/dev/null
	printf '%s  %s\n' "$(du -k "$OUT/$name.png" | cut -f1)K" "docs/features/$name.png"
}

# still NAME COLS ROWS "copal-tm options" [ring ...]
still() {
	name=$1 cols=$2 rows=$3 opts=$4
	shift 4
	wanted "${name%%-*}" || return 0
	# shellcheck disable=SC2086 # the options are words on purpose
	"$BIN" --simulate --frame="${cols}x$rows" $opts > "$WORK/$name.ansi"
	shoot "$name" "$cols" "$rows" "$@" < "$WORK/$name.ansi"
}

# keys "TOKENS": wN sleeps N seconds; ENTER, DOWN, SPACE are those keys;
# anything else is typed as it is.
keys() {
	for t in $1; do
		case $t in
		w*) sleep "${t#w}" ;;
		ENTER) printf '\r' ;;
		DOWN) printf '\033[B' ;;
		SPACE) printf ' ' ;;
		*) printf '%s' "$t" ;;
		esac
	done
}

# live NAME COLS ROWS "copal-tm options" "TOKENS" [ring ...]
live() {
	name=$1 cols=$2 rows=$3 opts=$4 seq=$5
	shift 5
	wanted "${name%%-*}" || return 0
	# The program asks the terminal for its size and waits a moment for an
	# answer that never comes here; keys typed during that wait would be
	# read as the answer, so the first token is always a pause.  The stream
	# is cut by timeout(1) with the last frame still on the screen.
	total=$(printf '%s\n' "$seq" | tr ' ' '\n' | awk '/^w/ { t += substr($0, 2) } END { print t + 1.5 }')
	{ keys "$seq"; sleep 2; } |
		timeout "$total" \
			script -q -f -E always \
			-c "stty rows $rows cols $cols; exec $BIN --simulate $opts" \
			"$WORK/$name.ansi" > /dev/null 2>&1 || true
	shoot "$name" "$cols" "$rows" "$@" < "$WORK/$name.ansi"
}

# ---- 1. the whole window ---------------------------------------------------

still 01-split 132 40 "" \
	'1@S Y S~20,13' '2@C P U   H I S T O R Y~70,13' '3@C P U   U S E~32,13' \
	'4@Memory Utilization~128,7' '5@Network~126,3' '6@P R O C E S S E S~128,9' \
	'7@SIMULATED  1.0s'

# ---- 2. the instrument panel -----------------------------------------------

still 02-dashboard 132 40 "--view dashboard" \
	'1@CPU  Batt Temp  GPU~20,12' '2@◈ Utilization  ◈ Temperature  ◈ Kernel' \
	'3@┊~12,9' '4@◀55°~4,1' '5@P▄▂▅▅▂▃ E▃▂▆▄' '6@◎ System Pressure~32,2' \
	'7@◆ ytq'

# ---- 3. the process browser ------------------------------------------------

still 03-browser 132 40 "--view browser" \
	'1@├─▾ nginx master process' '2:6,19,1,1' '3:12,19,1,1' '4:13,19,1,1' \
	'5@copal-tm' '6@-c node server.js' '7@CPU%▾'

still 04-collapsed 132 40 "--view browser --open collapsed" \
	'1@▸9 foot' '2@▸2 firefox'

live 05-filter 132 40 "--view browser" \
	"w1.5 / f i r e w0.4 ENTER w1.2" \
	'1@/fire' '2@firefox'

# ---- 4. the Inspector ------------------------------------------------------

still 06-inspector 132 40 "--view browser --open inspector" \
	'1@1 Attributes  2 Contents  3 Network  4 Tools  5 Access' \
	'2@service       OpenRC: nginx'

live 07-contents 132 40 "--view browser" \
	"w1.5 / 8 0 8 0 w0.4 ENTER w0.3 ENTER w0.4 2 w1.2" \
	'1@2 Contents' '2@open fds~60,1'

still 08-network 132 40 "--view browser --open network" \
	'1@3 Network' '2@listening~36,1' '3@per-process byte rates~70,1' \
	'4@LISTEN~11,3'

# ---- 5. acting: Services and the halt plan ----------------------------------

still 09-services 132 40 "--view browser --open services" \
	'1@Terminate~30,1' '2@trapped' '3@Kill ~50,1'

still 10-halt 132 40 "--view browser --open halt" \
	'1@the parent sh~72,1' '2@addressee~24,4' '3@kill -TERM -4820' '4@→ TERM~60,4' \
	'5@[Enter] send'

live 11-halt-service 132 40 "--view browser" \
	"w1.5 / m a s t e r w0.4 ENTER w0.3 x w1.2" \
	'1@nginx is a supervised~68,2' '2@owned by root~72,2' '3#2@rc-service nginx stop'

live 12-halt-dstate 132 40 "--view browser" \
	"w1.5 / s d b w0.4 ENTER w0.3 x w1.2" \
	'1@in uninterruptible sleep~72,3' '2@→ KILL~48,1'

live 13-halt-stopped 132 40 "--view browser" \
	"w1.5 / n o t e s w0.4 ENTER w0.3 x w1.2" \
	'1@stopped: it will not~72,2'

live 14-halt-refused 132 40 "--view browser" \
	"w1.5 / c g i w0.4 ENTER w0.3 x w1.2" \
	'1@owned by nginx'

live 15-tools 132 40 "--view browser" \
	"w1.5 / s t u b b o r n w0.4 ENTER w0.3 ENTER w0.4 4 w1.2" \
	'1@4 Tools' '2@✗ TERM~60,1' '3@wait 4.0s'

# ---- 6. the record, and the keys --------------------------------------------

# A short session, then T: a refusal, a mark, a pause and a faster tick, as
# the Transcript wrote them down.  q closes an overlay; it quits only from
# the main screen, and this sequence never sends it there.
live 16-transcript 132 40 "--view browser" \
	"w1.5 / m a s t e r w0.4 ENTER w0.3 x w0.5 q w0.3 SPACE w0.3 p w0.6 p w0.3 + w0.5 T w1.2" \
	'1@SIMULATED:~71,1' '2@nginx (310):~72,1' '3@paused~20,3'

still 17-help 132 40 "--open help"

# ---- 7. degrading legibly ---------------------------------------------------

still 18-narrow 78 24 ""

still 19-ascii-mono 100 30 "--charset ascii --theme mono"
