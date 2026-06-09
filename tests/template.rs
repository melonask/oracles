#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use oracles::provider::template::{TemplateVars, render_template};
use std::collections::BTreeMap;

#[test]
fn renders_template_values() -> oracles::Result<()> {
    let vars = TemplateVars::from([
        ("coin_id".to_owned(), "ethereum".to_owned()),
        ("quote_lower".to_owned(), "usd".to_owned()),
    ]);

    let rendered = render_template(
        "https://example.com/simple?ids={coin_id}&vs_currencies={quote_lower}",
        &vars,
    )?;

    assert_eq!(
        rendered,
        "https://example.com/simple?ids=ethereum&vs_currencies=usd"
    );

    Ok(())
}

#[test]
fn rejects_unknown_placeholder() {
    let vars = BTreeMap::new();
    assert!(render_template("https://example.com/{missing}", &vars).is_err());
}
