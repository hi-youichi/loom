use super::*;

#[test]
fn validate_frontmatter_basic() {
    let (name, desc, triggers, body) =
        validate_frontmatter("---\nname: foo\ndescription: bar\n---\nbody content").unwrap();
    assert_eq!(name, "foo");
    assert_eq!(desc, "bar");
    assert!(triggers.is_empty());
    assert_eq!(body, "body content");
}

#[test]
fn validate_frontmatter_with_triggers() {
    let (_, _, triggers, _) = validate_frontmatter(
        "---\nname: foo\ndescription: bar\ntriggers:\n  - rust error\n  - cargo build\n---\nbody",
    )
    .unwrap();
    assert_eq!(triggers, vec!["rust error", "cargo build"]);
}

#[test]
fn validate_frontmatter_multiline_body() {
    let (_, _, _, body) = validate_frontmatter(
        "---\nname: foo\ndescription: bar\n---\n# Heading\n\nStep 1.\nStep 2.\n",
    )
    .unwrap();
    assert!(body.contains("Step 1."));
    assert!(body.contains("Step 2."));
}

#[test]
fn validate_frontmatter_missing_name() {
    let err = validate_frontmatter("---\ndescription: only desc\n---\nbody").unwrap_err();
    assert!(err
        .warnings
        .iter()
        .any(|w| w.message.contains("must include 'name'")));
}

#[test]
fn validate_frontmatter_missing_description() {
    let err = validate_frontmatter("---\nname: foo\n---\nbody").unwrap_err();
    assert!(err
        .warnings
        .iter()
        .any(|w| w.message.contains("must include 'description'")));
}

#[test]
fn validate_frontmatter_no_frontmatter() {
    let err = validate_frontmatter("just text").unwrap_err();
    assert!(err
        .warnings
        .iter()
        .any(|w| w.message.contains("YAML frontmatter")));
}

#[test]
fn validate_frontmatter_unclosed_frontmatter() {
    let err =
        validate_frontmatter("---\nname: foo\ndescription: bar\nbody without closing").unwrap_err();
    assert!(err
        .warnings
        .iter()
        .any(|w| w.message.contains("not closed")));
}
