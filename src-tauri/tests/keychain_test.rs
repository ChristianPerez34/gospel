use gospel_lib::keychain;

#[test]
fn test_keychain_api_surface() {
    // Credentials persist to $XDG_CONFIG_HOME/gospel/credentials.json — isolate
    // the config home so probes don't write into the real credentials file.
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_CONFIG_HOME", dir.path());
    std::env::set_var("GOSPEL_KEYCHAIN_TEST", "1");
    std::env::remove_var("OPENAI_API_KEY");

    // Verify the API functions exist and return expected types
    let provider = "openai";

    // has_key should return false for unconfigured provider
    assert!(!keychain::has_key(provider));

    // store should return Ok for valid provider
    assert!(keychain::store(provider, "sk-test-123").is_ok());

    // has_key should return true after store
    assert!(keychain::has_key(provider));

    // retrieve should return the stored key
    assert_eq!(keychain::retrieve(provider).unwrap(), "sk-test-123");

    // delete should succeed
    let _ = keychain::delete(provider);

    // OAuth providers are supported provider IDs even when they do not use API keys.
    assert!(keychain::store("github_copilot", "unused-test-value").is_ok());
    let _ = keychain::delete("github_copilot");
    assert!(keychain::store("grok", "unused-test-value").is_ok());
    let _ = keychain::delete("grok");

    // Unsupported provider should error
    assert!(keychain::store("invalid_provider", "key").is_err());
}
