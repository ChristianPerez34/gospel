---
name: bounty-recon
description: Build a black-box web target threat model from in-scope HTTP recon, JS bundles, API specs, and auth flows.
---

# Bounty Recon

Goal: produce a target-specific threat model, not a generic checklist.

## Collect

- Landing page, robots.txt, sitemap.xml, `.well-known/`, and any linked API docs or OpenAPI spec.
- JS bundle URLs and source maps. Extract route names, API base paths, feature flags, role names, and error strings.
- Auth flows: login, logout, refresh, password reset, invite, MFA, OAuth callbacks. Note which steps change server state.
- Role and tenant boundaries: what identifies a user, an org, a workspace, a resource? Which identifiers are client-supplied?
- Error shapes: 401 vs 403 vs 404, validation errors, stack traces, request IDs.
- Rate-limit behavior on a single harmless endpoint. Do not probe limits aggressively.

## Write the threat model

Create `.gospel/bounty/runs/<run-id>/threat-model.md` with:

- **Assets**: what is worth stealing or changing?
- **Roles**: anonymous, user, admin, support, service, other tenants.
- **Trust boundaries**: browser/server, tenant/tenant, user/admin, service/service.
- **Auth model**: sessions, cookies, bearer tokens, CSRF defenses, refresh behavior.
- **Data flows**: where identifiers enter, where authorization is checked, where side effects happen.
- **Candidate classes**: the prompt classes most likely to pay off, with a reason for each.
- **Out of scope**: anything the program excludes or that you will not test.

Stop after writing the threat model and ask the user to review it before the broad hypothesis sweep.
