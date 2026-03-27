use crate::formatting::telegram::{escape_html, escape_markdown_v2, FormattedMessage};

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

#[test]
fn markdown_v2_message_escapes_plain_text_input() {
    let message = FormattedMessage::markdown_v2("Heading #1", "Heading #1");
    assert_eq!(message.text, "Heading \\#1");
    assert_eq!(message.plain_text_fallback, "Heading #1");
}

#[test]
fn markdown_v2_rendered_preserves_intentional_markup() {
    let message = FormattedMessage::markdown_v2_rendered("*Title*", "Title");
    assert_eq!(message.text, "*Title*");
    assert_eq!(message.plain_text_fallback, "Title");
}
