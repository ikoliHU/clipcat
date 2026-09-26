# glib 0.18.5 security backport

This is the unmodified crates.io `glib-0.18.5.crate`, except for two lines in
`src/variant_iter.rs` and this note. Archive SHA256:
`233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`.

Backported [gtk-rs/gtk-rs-core PR 1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343),
upstream merge `05dff0ee696f9bcd8617cd48c4b812d046d440cb`:
make the pointer mutable and pass `&mut p` to the C out-parameter, instead of
writing through a shared reference. This fixes the cause of
[RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)
without mixing incompatible GTK/glib generations in Tauri.

The Linux `glib_variant_string_iteration` regression test runs in optimized mode
in CI. The package version intentionally remains 0.18.5, so version-only advisory
scanners may still report it. Remove this override when Tauri's GTK stack supports
an upstream fixed version; do not suppress unrelated advisories.
