fn main() {
    // Generates Tauri's application manifest and codegen for the app binary.
    //
    // NOTE: this does NOT attach a Common-Controls-v6 manifest to the
    // `cargo test` runner. That runner is a plain lib-unittest executable, not
    // a Tauri binary, so it loads the legacy comctl32 v5 at startup and crashes
    // with STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139) on TaskDialogIndirect
    // (pulled in transitively by tauri/winit/windows-rs) — before main() runs.
    //
    // The fix lives in `.cargo/config.toml`: a Windows `runner` injects the
    // comctl32 v6 manifest into every executed test/bench binary via `mt.exe`
    // (see src-tauri/tools/inject_manifest.js). It is idempotent and survives
    // `cargo clean`, so no manual manifest step is needed.
    //
    // Do NOT also embed a manifest here via embed-resource: two manifests in one
    // binary collide at link time (CVT1100 "duplicate MANIFEST" -> LNK1123).
    tauri_build::build();
}
