#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use oracles::config::load_config;
use oracles::domain::ProviderId;

#[test]
#[cfg_attr(
    not(all(feature = "telegram", feature = "webhook")),
    ignore = "requires `telegram` and `webhook` features (Config.example.toml uses both)"
)]
fn config_example_loads() -> oracles::Result<()> {
    let config = load_config("Config.example.toml")?;

    assert_eq!(config.version, 1);
    assert!(!config.assets.is_empty());
    assert!(config.provider(&ProviderId::new("static")?).is_some());

    Ok(())
}
