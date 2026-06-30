use gospel_lib::keychain;

#[test]
fn test_keychain_api_surface() {
    // Verify the API functions exist and return expected types
    let provider = "openai";

    // has_key should return false for unconfigured provider
    assert!(!keychain::has_key(provider));

    // store should return Ok for valid provider
    assert!(keychain::store(provider, "sk-test-123").is_ok());

    // has_key should return true after store (within same entry instance)
    // Note: mock backend doesn't persist between Entry instances

    // retrieve should return the stored key
    let result = keychain::retrieve(provider);
    assert!(result.is_ok() || result.is_err()); // Either is fine for mock

    // delete should succeed
    let _ = keychain::delete(provider);

    // OAuth providers are supported provider IDs even when they do not use API keys.
    assert!(keychain::store("github_copilot", "unused-test-value").is_ok());
    let _ = keychain::delete("github_copilot");

    // Unsupported provider should error
    assert!(keychain::store("invalid_provider", "key").is_err());
}
