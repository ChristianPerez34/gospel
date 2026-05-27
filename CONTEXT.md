# Gospel Context

## Glossary

- **Credentialed Provider**: A supported model provider for which Gospel can access a usable credential source. API-key providers are credentialed when a key exists in the OS keychain. ChatGPT Plus/Pro is credentialed when a reusable OAuth session exists in the local ChatGPT auth cache.
- **Provider Visibility**: A non-secret user preference that determines whether a credentialed provider should contribute models to the model picker. Missing visibility data defaults to visible.
- **Available Model**: A backend-returned provider/model entry that is selectable because its provider is credentialed, visible, and model loading returned a live or cached model list.
- **Turn**: One LLM inference cycle, which may include tool execution. A turn ends when the LLM produces a final text response (not a tool call).
- **Conversation**: A sequence of user/agent message pairs within a session. Conversations are identified by session ID and stored server-side.
- **Tool**: A registered function the LLM can invoke during a turn. Tools execute and return results that feed back into the next turn.
