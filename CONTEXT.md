# Gospel Context

## Glossary

- **Credentialed Provider**: A supported model provider for which Gospel can access a usable credential source. API-key providers are credentialed when a key exists in the OS keychain. ChatGPT Plus/Pro is credentialed when a reusable OAuth session exists in the local ChatGPT auth cache.
- **Provider Visibility**: A non-secret user preference that determines whether a credentialed provider should contribute models to the model picker. Missing visibility data defaults to visible.
- **Available Model**: A backend-returned provider/model entry that is selectable because its provider is credentialed, visible, and model loading returned a live or cached model list.
