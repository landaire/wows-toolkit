#!/usr/bin/env bash

# Runs one command in a network namespace of its own.
#
# The hosted runner reports job status over the same network the build must not
# have, so isolation is scoped to the build instead of the machine: dropping the
# host's traffic takes the runner agent down with it.
#
# Every Buck step goes through here. Actions run in the namespace the daemon was
# spawned in, so a client that reattaches to a daemon started outside would run
# them with the network back.

set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "run-offline.sh needs a command to run." >&2
    exit 1
fi

# sudo resolves commands through secure_path and hands the child a scrubbed
# environment, so both are pinned here and restored past the privilege drop.
if ! program=$(command -v "$1"); then
    echo "run-offline.sh cannot find $1 on PATH." >&2
    exit 1
fi
shift

exec sudo -E unshare --net -- sh -c '
    ip link set lo up
    uid=$1; gid=$2; path=$3; home=$4
    shift 4
    exec setpriv --reuid="$uid" --regid="$gid" --init-groups env PATH="$path" HOME="$home" "$@"
' sh "$(id -u)" "$(id -g)" "$PATH" "$HOME" "$program" "$@"
