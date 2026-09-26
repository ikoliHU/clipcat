#!/bin/sh
# Runs in a disposable Linux container; the repository mount stays read-only.
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends libglib2.0-dev pkg-config ca-certificates >/dev/null
mkdir -p /tmp/clipcat-glib/tests
cp /source/src-tauri/tests/glib_backport.rs /tmp/clipcat-glib/tests/glib_backport.rs
cat > /tmp/clipcat-glib/Cargo.toml <<'EOF'
[package]
name = "clipcat-glib-regression"
version = "0.0.0"
edition = "2021"
[dependencies]
glib = "=0.18.5"
EOF
cd /tmp/clipcat-glib
if cargo test --release --test glib_backport --quiet > baseline.log 2>&1; then
    echo 'Unpatched GLib test passed on this compiler; this does not disprove upstream UB.'
elif grep -q 'signal: 11, SIGSEGV' baseline.log; then
    echo 'REPRODUCED: unpatched GLib string iterator crashes in optimized mode (SIGSEGV).'
else
    cat baseline.log
    exit 1
fi
cat >> Cargo.toml <<'EOF'
[patch.crates-io]
glib = { path = "/source/src-tauri/vendor/glib" }
EOF
cargo test --release --test glib_backport --quiet
