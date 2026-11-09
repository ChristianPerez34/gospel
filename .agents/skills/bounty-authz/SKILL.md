---
name: bounty-authz
description: Test authorization boundaries on black-box web targets: IDOR/BOLA, role escalation, tenant isolation, object rebinding, and inconsistent access paths.
---

# Bounty Authz

Authorization bugs are usually about identity, not payloads.

## Build the identity matrix

For every object you can name, record:

- The object identifier and where it comes from (path, query, body, header, cookie).
- Which auth profile created it.
- Which auth profiles can read, update, delete, share, or enumerate it.
- The expected rule (owner-only, org-only, admin-only, public).

Use at least two auth profiles. If only one is configured, ask the user for a second profile before claiming an authorization finding.

## Test

- Swap the object identifier between profiles. Compare status, body shape, and side effects, not just status codes.
- Reach the same object through a different route: nested resource, export, search, bulk endpoint, GraphQL node, admin alias.
- Change the object's owner/tenant field in an update request. Does the server trust it?
- Rebind an object to another user, then try to access it back.
- Check deleted, archived, transferred, and revoked objects.
- Check invite, share, transfer, and ownership-change flows for a window where both parties retain access.

## Evidence

Record the request/response evidence ID for each profile and each step. A finding needs the secure expectation, the observed behavior, and the smallest reproduction. Do not claim impact from a status-code difference alone.
