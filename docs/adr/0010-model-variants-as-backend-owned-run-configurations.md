# Model variants as backend-owned run configurations

Gospel treats a model variant as a backend-owned run configuration attached to a provider/model slug, and sessions persist only the optional variant ID rather than raw provider request parameters. This keeps provider-specific parameter shape out of the frontend and session export format, preserves parent-model availability and counting semantics, and lets stale variant IDs fall back safely to the default model behavior with a warning.

**Considered Options**

- Persist raw provider parameters in sessions and exports. Rejected because it would make old sessions depend on provider-specific payload shapes and expose low-level request configuration through user-facing data.
- Encode each variant as a synthetic model slug. Rejected because variants should not inflate availability counts or hide the shared provider/model identity.
- Let the frontend own variant parameter mapping. Rejected because the backend is the boundary that builds provider requests and can validate deprecated or missing variants consistently.

**Consequences**

Built-in variants must be added to the backend registry before the frontend can offer them. Deprecated variants can remain executable for existing sessions while being hidden from new selections.
