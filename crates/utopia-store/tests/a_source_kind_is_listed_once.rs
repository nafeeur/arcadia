//! #247: the kind of a source is defined in exactly one place, front end and
//! back end must agree.
//!
//! The backend `SourceKind` (utopia-core) is one enum feeding two lists: the
//! creation whitelist and the sync dispatch (the latter matches exhaustively
//! on the enum, so the compiler guarantees a new kind forces a decision on
//! how it syncs). The frontend copy lives in `web/src/sourceKinds.ts`; this
//! test reads it back and compares it against the enum — previously both
//! sides were hand-written, and five connectors made it into the UI and into
//! sync but not into the creation whitelist: pickable in the UI, rejected at
//! creation with "kind must be one of…". A drift neither unit tests nor tsc
//! can see, made visible here.
//!
//! No database needed.

use std::path::Path;
use utopia_core::models::SourceKind;

/// Reads the quoted literals out of `CREATABLE_SOURCE_KINDS = [ "…", … ] as const`, in order
fn frontend_kinds(src: &str) -> Vec<String> {
    let start = src
        .find("CREATABLE_SOURCE_KINDS = [")
        .expect("web/src/sourceKinds.ts declares CREATABLE_SOURCE_KINDS");
    let body = &src[start..];
    let end = body.find(']').expect("the array closes");
    body[..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

#[test]
fn the_frontend_list_matches_the_backend_enum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/sourceKinds.ts");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let frontend = frontend_kinds(&src);
    let backend: Vec<String> = SourceKind::creatable()
        .map(|k| k.as_str().to_string())
        .collect();
    assert_eq!(
        frontend, backend,
        "web/src/sourceKinds.ts and utopia_core::models::SourceKind list different kinds (order matters: it is the dialog's order)"
    );
}

#[test]
fn every_kind_round_trips_through_its_string() {
    for k in SourceKind::all() {
        assert_eq!(SourceKind::parse(k.as_str()), Some(k), "{k:?}");
    }
    assert_eq!(SourceKind::parse("watch_folder"), None);
    assert!(!SourceKind::Memory.creatable_by_hand());
    assert!(!SourceKind::Upload.creatable_by_hand());
    assert!(SourceKind::S3.creatable_by_hand());
}
