#!/bin/sh
# Arguments are passed as separate argv entries; never interpolate shell code.
set -eu
parent=$1; destination=$2; payload=$3; executable=$4; receipt=$5
version=$6; kind=$7; error=$8; expected=$9; package=${10}
ready=${11}
stage=$(dirname "$receipt")
backup="${destination}.sidedoor-backup"
next="${destination}.sidedoor-next"
replaced=false
owns_next=false
exited=false
new_pid=
restart() {
    SIDEDOOR_UPDATE_RECEIPT="$receipt" "$executable" >/dev/null 2>&1 &
    new_pid=$!
}
recover() {
    status=$?
    if [ "$status" -ne 0 ]; then
        printf '%s\n' 'The last update could not finish. Your previous installation was kept where possible; see last-update.log in the update cache.' > "$error"
        if [ -n "$new_pid" ]; then kill "$new_pid" 2>/dev/null || true; fi
        if [ "$replaced" = true ] && [ -d "$backup" ]; then
            rm -rf "$destination"
            mv "$backup" "$destination"
        fi
        unset SIDEDOOR_UPDATE_RECEIPT
        if [ "$exited" = true ] && [ -x "$executable" ]; then
            "$executable" >/dev/null 2>&1 &
        fi
    fi
    if [ "$owns_next" = true ]; then rm -rf "$next"; fi
    rm -rf "$stage"
}
trap recover EXIT
: > "$ready"
# Wait for the app to release its executable and stop bundled plugin processes.
count=0
while kill -0 "$parent" 2>/dev/null; do
    count=$((count + 1)); [ "$count" -le 60 ] || exit 1
    sleep 1
done
exited=true
if [ "$kind" = macos ]; then
    actual=$(shasum -a 256 "$package" | cut -d ' ' -f 1)
else
    actual=$(sha256sum "$package" | cut -d ' ' -f 1)
fi
[ "$actual" = "$expected" ]
rm -f "$error"
case "$kind" in
    macos|portable)
        # A prior backup is never overwritten: retain recovery evidence.
        [ ! -e "$backup" ] && [ ! -e "$next" ]
        owns_next=true
        if [ "$kind" = macos ]; then ditto "$payload" "$next"; codesign --verify --deep --strict "$next"
        else cp -R "$payload" "$next"; fi
        mv "$destination" "$backup"
        replaced=true
        mv "$next" "$destination"
        ;;
    deb) pkexec /usr/bin/apt-get install -y "$package" ;;
    rpm) pkexec /usr/bin/dnf install -y "$package" ;;
    *) exit 1 ;;
esac
restart
count=0
while [ "$(cat "$receipt" 2>/dev/null || true)" != "$version" ]; do
    kill -0 "$new_pid" 2>/dev/null || exit 1
    count=$((count + 1)); [ "$count" -le 60 ] || exit 1
    sleep 1
done
if [ "$replaced" = true ]; then rm -rf "$backup"; fi
echo "Updated to $version"
