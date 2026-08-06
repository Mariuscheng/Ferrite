use ferrite::tools::diff_ops::{
    apply_diff_hunks, parse_hunk_header, parse_unified_diff, strip_git_prefix, DiffHunk, DiffLine,
    DiffLineKind, ParsedDiff,
};
use std::path::PathBuf;

#[test]
fn parse_simple_unified_diff() {
    let patch = "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@\n fn main() {\n-    println!(\"hello\");\n+    println!(\"hello, world\");\n+    println!(\"extra\");\n }\n";
    let diffs = parse_unified_diff(patch).expect("parse");
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].new_path, "src/main.rs");
    assert_eq!(diffs[0].hunks.len(), 1);
    assert_eq!(diffs[0].hunks[0].old_start, 1);
    assert_eq!(diffs[0].hunks[0].old_count, 3);
    assert_eq!(diffs[0].hunks[0].new_start, 1);
    assert_eq!(diffs[0].hunks[0].new_count, 4);
}

#[test]
fn parse_multi_file_diff() {
    let patch = "--- a/foo.rs\n+++ b/foo.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n--- a/bar.rs\n+++ b/bar.rs\n@@ -1,1 +1,1 @@\n-xxx\n+yyy\n";
    let diffs = parse_unified_diff(patch).expect("parse");
    assert_eq!(diffs.len(), 2);
    assert_eq!(diffs[0].new_path, "foo.rs");
    assert_eq!(diffs[1].new_path, "bar.rs");
}

#[test]
fn apply_simple_hunk() {
    let original = "fn main() {\n    println!(\"hello\");\n}\n";
    let diff = ParsedDiff {
        original_path: "a/src/main.rs".into(),
        new_path: "src/main.rs".into(),
        hunks: vec![DiffHunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 4,
            header: "@@ -1,3 +1,4 @@".into(),
            lines: vec![
                DiffLine { kind: DiffLineKind::Context, text: "fn main() {".into() },
                DiffLine { kind: DiffLineKind::Removed, text: "    println!(\"hello\");".into() },
                DiffLine { kind: DiffLineKind::Added, text: "    println!(\"hello, world\");".into() },
                DiffLine { kind: DiffLineKind::Added, text: "    println!(\"extra\");".into() },
                DiffLine { kind: DiffLineKind::Context, text: "}".into() },
            ],
        }],
    };

    let result = apply_diff_hunks(original, &diff, 3, true, &PathBuf::from("test.rs"));
    assert!(result.preview.is_some());
    let preview = result.preview.unwrap();
    assert_eq!(preview.hunks_preview.len(), 1);
    assert!(preview.hunks_preview[0].applied);
}

#[test]
fn fuzz_matching_tolerates_shift() {
    let original = "// comment added later\nfn main() {\n    println!(\"hello\");\n}\n";
    let diff = ParsedDiff {
        original_path: "a/test.rs".into(),
        new_path: "test.rs".into(),
        hunks: vec![DiffHunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            header: "@@ -1,3 +1,3 @@".into(),
            lines: vec![
                DiffLine { kind: DiffLineKind::Context, text: "fn main() {".into() },
                DiffLine { kind: DiffLineKind::Removed, text: "    println!(\"hello\");".into() },
                DiffLine { kind: DiffLineKind::Added, text: "    println!(\"goodbye\");".into() },
                DiffLine { kind: DiffLineKind::Context, text: "}".into() },
            ],
        }],
    };

    let result = apply_diff_hunks(original, &diff, 3, true, &PathBuf::from("test.rs"));
    let preview = result.preview.unwrap();
    // Should match despite the added comment line
    assert!(preview.hunks_preview[0].applied);
}

#[test]
fn hunk_header_parsing() {
    assert_eq!(
        parse_hunk_header("@@ -1,3 +1,4 @@").unwrap(),
        (1, 3, 1, 4)
    );
    assert_eq!(
        parse_hunk_header("@@ -10 +10,0 @@").unwrap(),
        (10, 1, 10, 0)
    );
}

#[test]
fn strip_git_prefix_works() {
    assert_eq!(strip_git_prefix("a/src/main.rs"), "src/main.rs");
    assert_eq!(strip_git_prefix("b/src/lib.rs"), "src/lib.rs");
    assert_eq!(strip_git_prefix("src/main.rs"), "src/main.rs");
}