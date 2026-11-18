#!/usr/bin/env bash
set -Eeuo pipefail

binary=$1
report_dir=$2
profile_root=$3
seed=$4
report_file="$report_dir/native.json"
observation="$report_dir/ui-observation.json"
status=Fail
detail="The Linux native input driver did not finish."
pid=''
window=''
first_pid=''
restart_pid=''
clipboard_ready=false
clipboard_touched=false
clipboard_backup=''
clipboard_known=''
events="$report_dir/native-actions.jsonl"
: > "$events"

record() {
    jq -nc --arg action "$1" --argjson observed "$2" '{action:$action, observed:$observed}' >> "$events"
}

cleanup() {
    if $clipboard_ready && $clipboard_touched && clipboard_contains "$clipboard_known"; then
        if ! printf '%s' "$clipboard_backup" | xclip -selection clipboard -in >/dev/null 2>&1; then
            status=Fail
            detail='Could not restore the original X11 clipboard text.'
        fi
    fi
    unset clipboard_backup
    if [[ -n $pid ]] && kill -0 "$pid" 2>/dev/null; then
        if [[ -n $window ]] && command -v import >/dev/null; then
            timeout 4s import -window "$window" "$report_dir/failure.png" 2>/dev/null || true
        fi
        kill "$pid" 2>/dev/null || true
        if ! wait_for "test process cleanup" 5 process_finished; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
        wait "$pid" 2>/dev/null || true
    fi
    if command -v jq >/dev/null; then
        jq -n --arg status "$status" --arg detail "$detail" --arg seed "$seed" \
            --arg kernel "$(uname -sr)" --arg arch "$(uname -m)" \
            --arg session "${XDG_SESSION_TYPE:-unknown}" --arg wsl "${WSL_DISTRO_NAME:-}" \
            --arg first_pid "$first_pid" --arg restart_pid "$restart_pid" \
            --slurpfile actions "$events" \
            '{schema:1,status:$status,detail:$detail,driver:"X11 xdotool, xclip, ImageMagick",seed:$seed,kernel:$kernel,architecture:$arch,session_type:$session,wsl_distro:$wsl,app_pid:$first_pid,restarted_pid:$restart_pid,actions:$actions}' > "$report_file"
    else
        printf '{"schema":1,"status":"Blocked","detail":"Install jq before running the Linux X11 driver."}\n' > "$report_file"
    fi
    printf '%s. %s\n' "$status" "$detail"
}

blocked() {
    status=Blocked
    detail=$1
    exit 78
}

fail() {
    detail=$1
    exit 1
}

wait_for() {
    local description=$1 max_seconds=$2
    shift 2
    local deadline=$((SECONDS + max_seconds))
    until "$@"; do
        if (( SECONDS >= deadline )); then
            printf 'Timed out waiting for %s.\n' "$description" >&2
            return 1
        fi
        sleep 0.04
    done
}

trap cleanup EXIT
trap 'exit 1' INT TERM
for command_name in jq xdotool xclip import timeout rg; do
    command -v "$command_name" >/dev/null || blocked "The Linux X11 driver needs $command_name."
done

[[ -n ${DISPLAY:-} ]] || blocked 'The Linux driver needs a real X11 display.'
timeout 2s xdotool getdisplaygeometry >/dev/null 2>&1 || blocked 'Cannot connect to the configured X11 desktop.'

clipboard_targets=$(timeout 2s xclip -selection clipboard -target TARGETS -out 2>/dev/null) || clipboard_targets=''
if [[ -n $clipboard_targets ]]; then
    while IFS= read -r target; do
        case $target in
            ''|TARGETS|TIMESTAMP|MULTIPLE|SAVE_TARGETS|UTF8_STRING|STRING|TEXT|text/plain|text/plain\;charset=utf-8) ;;
            *) blocked 'The current X11 clipboard contains formats the driver cannot restore. Use an isolated desktop or replace the clipboard with test text.' ;;
        esac
    done <<< "$clipboard_targets"
    clipboard_backup=$(timeout 2s xclip -selection clipboard -out 2>/dev/null && printf '.') || \
        blocked 'The current X11 clipboard text could not be preserved.'
    clipboard_backup=${clipboard_backup%.}
fi
clipboard_ready=true

state_matches() {
    [[ -f $observation ]] && jq -e "$1" "$observation" >/dev/null 2>&1
}

stable_layout_matches() {
    local signature
    if ! state_matches "$1"; then
        stable_signature=''
        stable_samples=0
        return 1
    fi
    signature=$(jq -ec '[.outer_ratio,.context_ratio,.bounds.outer_divider,.bounds.context_divider]' "$observation") || return 1
    if [[ $signature != "$stable_signature" ]]; then
        stable_signature=$signature
        stable_samples=0
    fi
    stable_samples=$((stable_samples + 1))
    (( stable_samples >= 5 ))
}

wait_for_settled_layout() {
    local stable_signature='' stable_samples=0
    wait_for "$1" 8 stable_layout_matches "$2"
}

process_finished() {
    ! kill -0 "$pid" 2>/dev/null
}

window_ready() {
    if process_finished; then
        fail 'The application exited before the fixture and native X11 window were ready.'
    fi
    state_matches '.ready == true' || return 1
    window=$(xdotool search --onlyvisible --pid "$pid" 2>/dev/null | head -n 1) || true
    [[ -n $window ]]
}

launch() {
    local profile=$1
    mkdir "$profile"
    env -u WAYLAND_DISPLAY THREADLINE_PROFILE_DIR="$profile" THREADLINE_TEST_SEED="$seed" \
        "$binary" --e2e-report "$observation" > "$report_dir/app-$2.log" 2>&1 &
    pid=$!
    window=''
    wait_for 'fixture and real X11 window' 20 window_ready || fail 'The application did not present a real X11 window.'
    if ! timeout 3s xdotool windowactivate --sync "$window"; then
        timeout 3s xdotool windowfocus --sync "$window" || blocked 'X11 desktop did not allow foreground focus.'
    fi
    [[ $(xdotool getwindowfocus) == "$window" ]] || blocked 'The Threadline window did not receive X11 keyboard focus.'
}

key() {
    require_focus
    xdotool key --clearmodifiers "$@"
}

require_focus() {
    [[ $(xdotool getwindowfocus) == "$window" ]] || blocked 'The Threadline window lost X11 keyboard focus.'
}

coordinate_pair() {
    jq -er "$1 | map(round) | @tsv" "$observation"
}

drag_between() {
    local start_x=$1 start_y=$2 end_x=$3 end_y=$4 step
    require_focus
    xdotool mousemove --window "$window" "$start_x" "$start_y" mousedown 1
    for step in {1..12}; do
        xdotool mousemove --window "$window" "$((start_x + (end_x - start_x) * step / 12))" \
            "$((start_y + (end_y - start_y) * step / 12))"
        sleep 0.016
    done
    xdotool mouseup 1
}

clipboard_contains() {
    local text
    text=$(read_clipboard) || return 1
    [[ $text == *"$1"* ]]
}

read_clipboard() {
    timeout 1s xclip -selection clipboard -out 2>/dev/null && printf '.'
}

clipboard_changed_to() {
    local text
    text=$(read_clipboard) || return 1
    [[ $text != "$2" && $text == *"$1"* ]]
}

copy_from_app() {
    local before
    before=$(read_clipboard) || before=''
    key ctrl+c
    wait_for "new native clipboard content" 4 clipboard_changed_to "$1" "$before" || fail "$2"
    clipboard_touched=true
    clipboard_known=$1
}

quit_app() {
    key ctrl+q
    wait_for 'process exit after Ctrl+Q' 8 process_finished || fail 'The native quit action did not exit the process.'
    wait "$pid" || fail 'The application returned an error after quit.'
    pid=''
}

launch "$profile_root/initial" initial
first_pid=$pid
awk 'NF >= 6 && $6 ~ "^/" {n=split($6, parts, "/");print parts[n]}' "/proc/$pid/maps" | sort -u > "$report_dir/native-modules.txt"
if rg -iq '^(lib)?(webkit|wry|electron|cef|chromium|msedgewebview2)' "$report_dir/native-modules.txt"; then
    fail 'The native process loaded a browser runtime module.'
fi
state_matches '.left_tab == 41 and .right_tab == 72 and .context_ratio >= 0.30 and .context_ratio <= 0.36' || \
    fail 'Expected fixture tabs and the one-third context pane at launch.'
record 'Launch native window' "$(jq -c '{left_tab,right_tab,context_ratio,scale_factor}' "$observation")"
timeout 4s import -window "$window" "$report_dir/launch.png"

key ctrl+1
wait_for 'left pane focus' 8 state_matches '.focused_pane == "left"' || fail 'The left pane did not receive focus.'
read -r x1 y1 x2 y2 < <(coordinate_pair '.scale_factor as $s | .bounds.markdown_copy | [(.[0]+1)*$s,(.[1]+.[3]/2)*$s,(.[0]+.[2]-1)*$s,(.[1]+.[3]/2)*$s]')
drag_between "$x1" "$y1" "$x2" "$y2"
copy_from_app 'Offline review' 'Pointer-selected Markdown did not change the native clipboard as expected.'
record 'Select and copy formatted Markdown' '"Synthetic Markdown token arrived on the X11 clipboard"'

key ctrl+2
wait_for 'right pane focus' 8 state_matches '.focused_pane == "right"' || fail 'The right pane did not receive focus.'
key ctrl+r
wait_for 'scheduled preview' 4 state_matches '.preview_state == "loading"' || fail 'The background preview did not start.'
key ctrl+f
require_focus
xdotool type --clearmodifiers 'native draft 72'
wait_for 'real review input' 4 state_matches '.right_input == "native draft 72" and .left_input == ""' || \
    fail 'Native text input did not update the intended review control.'
draft_preview=$(jq -r '.preview_state' "$observation")
[[ $draft_preview == 'loading' ]] || fail 'Review input was not observed before planned background completion.'
wait_for 'completed preview' 4 state_matches '.preview_state == "ready"' || fail 'The background preview did not finish.'
key ctrl+a
copy_from_app 'native draft 72' 'The native review input did not change the clipboard as expected.'
record 'Type and copy after activating the right pane while background preview is pending' "$(jq -c '{right_tab,right_input,left_input}' "$observation")"

key ctrl+Tab
wait_for 'second PR tab and the independent left tab' 8 state_matches '.right_tab == 68 and .left_tab == 41' || \
    fail 'The tabs did not switch independently.'
key ctrl+Tab
wait_for 'original PR tab and draft' 8 state_matches '.right_tab == 72 and .right_input == "native draft 72"' || \
    fail 'Returning to the original tab did not preserve the draft.'
record 'Change active PR tab and return to the draft' '72'

key ctrl+shift+l
wait_for 'linked issue context' 8 state_matches '.context_mode == "issue" and .right_tab == 72' || fail 'The context toggle did not show the linked issue.'
record 'Choose linked issue context' '"issue"'

outer_before=$(jq -er '.outer_ratio' "$observation")
read -r x1 y1 x2 y2 < <(coordinate_pair '.scale_factor as $s | .bounds.outer_divider | [(.[0]+.[2]/2)*$s,(.[1]+.[3]/2)*$s,(.[0]-62)*$s,(.[1]+.[3]/2)*$s]')
drag_between "$x1" "$y1" "$x2" "$y2"
wait_for_settled_layout 'settled outer splitter movement' "(.outer_ratio - $outer_before | fabs) > 0.025" || fail 'Dragging the workspace divider did not resize its panes.'
record 'Resize workspace divider' "$(jq -er '.outer_ratio' "$observation")"

inner_before=$(jq -er '.context_ratio' "$observation")
read -r x1 y1 x2 y2 < <(coordinate_pair '.scale_factor as $s | .bounds.context_divider | [(.[0]+.[2]/2)*$s,(.[1]+.[3]/2)*$s,(.[0]+48)*$s,(.[1]+.[3]/2)*$s]')
drag_between "$x1" "$y1" "$x2" "$y2"
wait_for_settled_layout 'settled context splitter movement' "(.context_ratio - $inner_before | fabs) > 0.025" || fail 'Dragging the context divider did not resize PR context.'
record 'Resize context divider' "$(jq -er '.context_ratio' "$observation")"

timeout 4s import -window "$window" "$report_dir/interaction.png"
if clipboard_contains "$clipboard_known"; then
    printf '%s' "$clipboard_backup" | xclip -selection clipboard -in >/dev/null 2>&1 || fail 'Could not restore the original X11 clipboard text.'
    clipboard_touched=false
fi
quit_app
record 'Quit with the native keyboard shortcut' '0'

observation="$report_dir/restart-observation.json"
launch "$profile_root/restart" restart
restart_pid=$pid
quit_app
record 'Relaunch a fresh isolated feasibility profile and quit again' "$restart_pid"

status=Pass
detail="Exercised the X11 window, Markdown selection, independent focus and tabs, review input, and both splitters"
