#!/bin/sh
# Test-only macOS transport. Production never selects this file.
set -eu
while [ "$#" -gt 0 ]; do
    case "$1" in
        --) shift; break ;;
        --ro|--rox|--rw|--rwx|--env|--bind-tcp|--connect-tcp|--log-level) shift; shift ;;
        -*) shift ;;
        *) break ;;
    esac
done
exec "$@"
