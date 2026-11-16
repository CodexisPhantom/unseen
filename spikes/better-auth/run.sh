#!/usr/bin/env bash
# Rebuild and restart the spike server in the Linux container.
set -e
cd "$(dirname "$0")"
V="-v $(pwd -W 2>/dev/null || pwd):/spike -v unseen-spike-target:/target -v unseen-spike-cargo:/root/.cargo/registry"
export MSYS_NO_PATHCONV=1
docker run --rm $V unseen-spike-auth cargo build 2>&1 | grep -E "^(error|warning: unused)|^ *-->|Finished" -A6 | head -60
docker rm -f spike-auth >/dev/null 2>&1 || true
docker run -d --name spike-auth -p 7979:7979 -e DATABASE_URL="sqlite:///tmp/spike.db?mode=rwc" $V unseen-spike-auth /target/debug/spike-better-auth >/dev/null
for i in $(seq 1 20); do curl -s -o /dev/null http://localhost:7979/api/auth/ok && break; sleep 0.5; done
