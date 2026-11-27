#!/bin/sh
# Runs Unseenarr as PUID:PGID with UMASK (docs/10-configuration.md).
set -e

data_dir="${UNSEENARR_DATA_DIR:-/config}"
mkdir -p "$data_dir"

if [ "$(id -u)" = "0" ]; then
    # Started as root: give the data directory to the target user, then drop root.
    chown -R "$PUID:$PGID" "$data_dir"
    umask "$UMASK"
    exec setpriv --reuid "$PUID" --regid "$PGID" --clear-groups unseenarr "$@"
fi

# Started with `--user`: PUID and PGID do not apply.
umask "$UMASK"
exec unseenarr "$@"
