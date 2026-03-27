use crate::formatting::telegram::{escape_html, escape_markdown_v2};

#[test]
fn escapes_markdown_v2_reserved_chars() {
    let input = "_*[]()~`>#+-=|{}.!";
    let escaped = escape_markdown_v2(input);
    assert_eq!(escaped, "\\_\\*\\[\\]\\(\\)\\~\\`\\>\\#\\+\\-\\=\\|\\{\\}\\.\\!");
}

#[test]
fn escapes_html_reserved_chars() {
    let input = "<tag>&value>";
    let escaped = escape_html(input);
    assert_eq!(escaped, "&lt;tag&gt;&amp;value&gt;");
}
