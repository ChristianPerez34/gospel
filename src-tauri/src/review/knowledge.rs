pub const SAST_RULES: &str = r##"
[
  {
    "id": "py/sql-injection",
    "description": "SQL injection via Python string formatting or concatenation",
    "cwe_id": "CWE-89",
    "cwe_name": "SQL Injection",
    "severity": "High",
    "category": "sql-injection",
    "pattern": "User-controlled data is interpolated into SQL text before execution.",
    "bad_example": "cursor.execute(\"SELECT * FROM users WHERE id = \" + user_id)",
    "good_example": "cursor.execute(\"SELECT * FROM users WHERE id = %s\", (user_id,))",
    "remediation": "Use parameterized queries or ORM bind parameters."
  },
  {
    "id": "py/command-injection",
    "description": "OS command injection through subprocess or shell execution",
    "cwe_id": "CWE-78",
    "cwe_name": "OS Command Injection",
    "severity": "Critical",
    "category": "command-injection",
    "pattern": "User input reaches os.system, subprocess with shell=True, or shell command strings.",
    "bad_example": "subprocess.run(\"tar xf \" + filename, shell=True)",
    "good_example": "subprocess.run([\"tar\", \"xf\", filename], check=True)",
    "remediation": "Avoid shell=True and pass arguments as an argv array after validation."
  },
  {
    "id": "py/path-traversal",
    "description": "Path traversal via unsanitized user-controlled paths",
    "cwe_id": "CWE-22",
    "cwe_name": "Path Traversal",
    "severity": "High",
    "category": "path-traversal",
    "pattern": "Request parameters are joined to a base directory without canonicalization and containment checks.",
    "bad_example": "open(os.path.join(base_dir, request.args[\"name\"]))",
    "good_example": "path = safe_join(base_dir, request.args[\"name\"])",
    "remediation": "Canonicalize the resolved path and verify it remains inside the allowed root."
  },
  {
    "id": "py/hardcoded-credentials",
    "description": "Hardcoded passwords, tokens, or API keys in Python code",
    "cwe_id": "CWE-798",
    "cwe_name": "Hard-coded Credentials",
    "severity": "High",
    "category": "secrets",
    "pattern": "Credential-looking literals are assigned to password, token, key, or secret variables.",
    "bad_example": "API_KEY = \"sk_live_123\"",
    "good_example": "API_KEY = os.environ[\"API_KEY\"]",
    "remediation": "Load secrets from a secure runtime secret store or environment variable."
  },
  {
    "id": "py/insecure-deserialization",
    "description": "pickle or marshal deserialization of untrusted data",
    "cwe_id": "CWE-502",
    "cwe_name": "Deserialization of Untrusted Data",
    "severity": "Critical",
    "category": "deserialization",
    "pattern": "Untrusted request, file, queue, or network data reaches pickle.loads or pickle.load.",
    "bad_example": "obj = pickle.loads(request.data)",
    "good_example": "obj = json.loads(request.data)",
    "remediation": "Use a safe data format and validate the decoded schema."
  },
  {
    "id": "py/insecure-random",
    "description": "Use of predictable random values for security decisions",
    "cwe_id": "CWE-330",
    "cwe_name": "Insufficiently Random Values",
    "severity": "Medium",
    "category": "randomness",
    "pattern": "random.random, random.choice, or time-based values create tokens, passwords, or nonces.",
    "bad_example": "token = ''.join(random.choice(chars) for _ in range(32))",
    "good_example": "token = secrets.token_urlsafe(32)",
    "remediation": "Use secrets or a cryptographically secure random number generator."
  },
  {
    "id": "py/ssrf",
    "description": "Server-side request forgery through user-controlled URLs",
    "cwe_id": "CWE-918",
    "cwe_name": "Server-Side Request Forgery",
    "severity": "High",
    "category": "ssrf",
    "pattern": "User input controls requests.get, httpx, urllib, or aiohttp targets.",
    "bad_example": "requests.get(request.args[\"url\"])",
    "good_example": "requests.get(allowlisted_url_for(request.args[\"id\"]))",
    "remediation": "Resolve the target from trusted identifiers and block private/link-local addresses."
  },
  {
    "id": "py/xxe",
    "description": "XML external entity parsing in Python",
    "cwe_id": "CWE-611",
    "cwe_name": "XML External Entity Reference",
    "severity": "High",
    "category": "xxe",
    "pattern": "Untrusted XML is parsed by libraries configured to resolve entities or network DTDs.",
    "bad_example": "doc = lxml.etree.fromstring(request.data)",
    "good_example": "parser = lxml.etree.XMLParser(resolve_entities=False, no_network=True)",
    "remediation": "Disable external entity resolution and network access for XML parsers."
  },
  {
    "id": "js/sql-injection",
    "description": "SQL injection in JavaScript or Node query construction",
    "cwe_id": "CWE-89",
    "cwe_name": "SQL Injection",
    "severity": "High",
    "category": "sql-injection",
    "pattern": "req/query/body values are embedded into SQL template strings or concatenated SQL.",
    "bad_example": "db.query(`SELECT * FROM users WHERE id = ${req.query.id}`)",
    "good_example": "db.query(\"SELECT * FROM users WHERE id = ?\", [req.query.id])",
    "remediation": "Use placeholders, query builders, or ORM bind parameters."
  },
  {
    "id": "js/command-injection",
    "description": "Command injection through child_process exec",
    "cwe_id": "CWE-78",
    "cwe_name": "OS Command Injection",
    "severity": "Critical",
    "category": "command-injection",
    "pattern": "User input reaches child_process.exec, execSync, or shell command templates.",
    "bad_example": "exec(`convert ${req.body.file} out.png`)",
    "good_example": "execFile(\"convert\", [safeFile, \"out.png\"])",
    "remediation": "Use execFile/spawn with argv arrays and validate file names."
  },
  {
    "id": "js/xss",
    "description": "Cross-site scripting through unsanitized HTML sinks",
    "cwe_id": "CWE-79",
    "cwe_name": "Cross-site Scripting",
    "severity": "High",
    "category": "xss",
    "pattern": "User-controlled strings reach innerHTML, dangerouslySetInnerHTML, document.write, or template HTML.",
    "bad_example": "element.innerHTML = req.query.message",
    "good_example": "element.textContent = req.query.message",
    "remediation": "Use text sinks or sanitize HTML with a proven sanitizer."
  },
  {
    "id": "js/open-redirect",
    "description": "Open redirect through user-controlled redirect targets",
    "cwe_id": "CWE-601",
    "cwe_name": "Open Redirect",
    "severity": "Medium",
    "category": "open-redirect",
    "pattern": "Request input controls res.redirect, window.location, or Location headers.",
    "bad_example": "res.redirect(req.query.next)",
    "good_example": "res.redirect(resolveAllowedReturnPath(req.query.next))",
    "remediation": "Allow only relative paths or map trusted route identifiers to URLs."
  },
  {
    "id": "js/insecure-deserialization",
    "description": "Code execution through eval-like deserialization",
    "cwe_id": "CWE-502",
    "cwe_name": "Deserialization of Untrusted Data",
    "severity": "Critical",
    "category": "deserialization",
    "pattern": "Untrusted serialized data reaches eval, Function, vm.runIn*, or unsafe object hydration.",
    "bad_example": "const obj = eval('(' + req.body.payload + ')')",
    "good_example": "const obj = JSON.parse(req.body.payload)",
    "remediation": "Use JSON.parse with schema validation and avoid code evaluation."
  },
  {
    "id": "js/prototype-pollution",
    "description": "Prototype pollution through recursive merge or path setters",
    "cwe_id": "CWE-1321",
    "cwe_name": "Prototype Pollution",
    "severity": "High",
    "category": "prototype-pollution",
    "pattern": "User-controlled object keys such as __proto__, constructor, or prototype are merged into objects.",
    "bad_example": "merge(config, JSON.parse(req.body.options))",
    "good_example": "merge(config, rejectPrototypeKeys(req.body.options))",
    "remediation": "Reject prototype keys and use merge libraries that defend against pollution."
  },
  {
    "id": "js/jwt-none",
    "description": "JWT signature verification disabled or weakened",
    "cwe_id": "CWE-347",
    "cwe_name": "Improper Verification of Cryptographic Signature",
    "severity": "Critical",
    "category": "token-verification",
    "pattern": "JWTs are decoded without verification or algorithms include none.",
    "bad_example": "const claims = jwt.decode(token)",
    "good_example": "const claims = jwt.verify(token, publicKey, { algorithms: [\"RS256\"] })",
    "remediation": "Require signature verification, expected issuer/audience, and explicit algorithms."
  },
  {
    "id": "rs/sql-injection",
    "description": "SQL injection in Rust query construction",
    "cwe_id": "CWE-89",
    "cwe_name": "SQL Injection",
    "severity": "High",
    "category": "sql-injection",
    "pattern": "Untrusted values are interpolated into sqlx, rusqlite, diesel sql_query, or raw SQL strings.",
    "bad_example": "let q = format!(\"SELECT * FROM users WHERE id = {}\", id);",
    "good_example": "sqlx::query(\"SELECT * FROM users WHERE id = ?\").bind(id)",
    "remediation": "Use bind parameters or typed query builders."
  },
  {
    "id": "rs/path-traversal",
    "description": "Path traversal in Rust file operations",
    "cwe_id": "CWE-22",
    "cwe_name": "Path Traversal",
    "severity": "High",
    "category": "path-traversal",
    "pattern": "User-controlled paths are joined to a root and read or written without canonical containment checks.",
    "bad_example": "let path = root.join(req.path); tokio::fs::read(path).await?",
    "good_example": "let path = checked_child_path(root, req.path)?;",
    "remediation": "Canonicalize, reject absolute/parent components, and enforce root containment."
  },
  {
    "id": "rs/weak-crypto",
    "description": "Broken or obsolete cryptographic algorithms",
    "cwe_id": "CWE-327",
    "cwe_name": "Broken or Risky Cryptographic Algorithm",
    "severity": "Medium",
    "category": "weak-crypto",
    "pattern": "MD5, SHA1, DES, RC4, or hand-rolled crypto is used for security-sensitive integrity or secrecy.",
    "bad_example": "let digest = md5::compute(password);",
    "good_example": "let hash = argon2.hash_password(password.as_bytes(), &salt)?;",
    "remediation": "Use modern password hashing, AEAD encryption, and maintained crypto libraries."
  },
  {
    "id": "generic/missing-authz",
    "description": "Missing authorization check before privileged action",
    "cwe_id": "CWE-862",
    "cwe_name": "Missing Authorization",
    "severity": "High",
    "category": "authorization",
    "pattern": "Handlers update, delete, export, or reveal tenant/user resources without checking ownership or permissions.",
    "bad_example": "deleteInvoice(req.params.id)",
    "good_example": "deleteInvoiceForUser(req.user.id, req.params.id)",
    "remediation": "Enforce object-level authorization at the server boundary."
  },
  {
    "id": "generic/unrestricted-upload",
    "description": "Unrestricted upload of dangerous files",
    "cwe_id": "CWE-434",
    "cwe_name": "Unrestricted File Upload",
    "severity": "High",
    "category": "file-upload",
    "pattern": "Uploaded file names, extensions, MIME types, or storage paths are trusted without validation.",
    "bad_example": "save(upload.filename, upload.bytes)",
    "good_example": "save(generatedName, bytesAfterTypeAndSizeValidation)",
    "remediation": "Generate server-side names, validate content, limit size, and store outside executable paths."
  }
]
"##;

pub const CWE_ENTRIES: &str = r##"
[
  {
    "id": "CWE-20",
    "name": "Improper Input Validation",
    "category": "input",
    "description": "Input is accepted without validating type, range, format, or business invariants.",
    "severity": "Medium",
    "detection_indicators": ["request values used directly", "missing schema validation", "unchecked parsing"],
    "remediation_steps": ["Validate at trust boundaries", "Reject invalid input", "Use typed schemas"],
    "examples": {"vulnerable": "process(req.body.amount)", "secure": "process(validated.amount)"}
  },
  {
    "id": "CWE-22",
    "name": "Path Traversal",
    "category": "path",
    "description": "External input controls a file path outside the intended directory.",
    "severity": "High",
    "detection_indicators": ["path join with request input", "../ accepted", "no canonical containment check"],
    "remediation_steps": ["Reject absolute and parent components", "Canonicalize paths", "Check root containment"],
    "examples": {"vulnerable": "read(root.join(user_path))", "secure": "read(checked_child_path(root, user_path)?)"}
  },
  {
    "id": "CWE-74",
    "name": "Injection",
    "category": "injection",
    "description": "Untrusted data is embedded into a command or interpreter input.",
    "severity": "High",
    "detection_indicators": ["string-built command", "string-built query", "template with user input"],
    "remediation_steps": ["Separate data from commands", "Use bind parameters", "Validate allowlisted tokens"],
    "examples": {"vulnerable": "run(\"tool \" + arg)", "secure": "run_tool([arg])"}
  },
  {
    "id": "CWE-78",
    "name": "OS Command Injection",
    "category": "injection",
    "description": "Untrusted input can alter an operating system command.",
    "severity": "Critical",
    "detection_indicators": ["shell=True", "child_process.exec", "os.system", "Command through sh -c"],
    "remediation_steps": ["Avoid shells", "Use argv APIs", "Validate each argument"],
    "examples": {"vulnerable": "exec(\"git \" + branch)", "secure": "execFile(\"git\", [branch])"}
  },
  {
    "id": "CWE-79",
    "name": "Cross-site Scripting",
    "category": "web",
    "description": "Untrusted input is rendered as executable browser content.",
    "severity": "High",
    "detection_indicators": ["innerHTML", "dangerouslySetInnerHTML", "document.write", "HTML template interpolation"],
    "remediation_steps": ["Use text sinks", "Escape contextually", "Sanitize trusted HTML"],
    "examples": {"vulnerable": "el.innerHTML = userText", "secure": "el.textContent = userText"}
  },
  {
    "id": "CWE-89",
    "name": "SQL Injection",
    "category": "injection",
    "description": "Untrusted data changes the structure of a SQL query.",
    "severity": "High",
    "detection_indicators": ["SQL concatenation", "template SQL", "format! SQL", "raw query with request input"],
    "remediation_steps": ["Use parameters", "Use query builders", "Avoid string-built SQL"],
    "examples": {"vulnerable": "query(\"SELECT * FROM u WHERE id=\" + id)", "secure": "query(\"SELECT * FROM u WHERE id=?\", [id])"}
  },
  {
    "id": "CWE-94",
    "name": "Code Injection",
    "category": "injection",
    "description": "Untrusted input is evaluated as code.",
    "severity": "Critical",
    "detection_indicators": ["eval", "Function constructor", "vm.runInContext", "dynamic import from input"],
    "remediation_steps": ["Remove code evaluation", "Use declarative formats", "Allowlist expressions if unavoidable"],
    "examples": {"vulnerable": "eval(req.body.code)", "secure": "executeAllowedAction(req.body.action)"}
  },
  {
    "id": "CWE-117",
    "name": "Improper Output Neutralization for Logs",
    "category": "injection",
    "description": "Untrusted data can forge or corrupt logs.",
    "severity": "Low",
    "detection_indicators": ["raw request data in logs", "newline-capable log fields", "audit logs built by concatenation"],
    "remediation_steps": ["Encode control characters", "Use structured logs", "Bound log field sizes"],
    "examples": {"vulnerable": "log(\"user=\" + user)", "secure": "log_json({\"user\": user})"}
  },
  {
    "id": "CWE-125",
    "name": "Out-of-bounds Read",
    "category": "memory",
    "description": "Code reads outside the intended memory range.",
    "severity": "High",
    "detection_indicators": ["unchecked index", "unsafe pointer arithmetic", "length from untrusted input"],
    "remediation_steps": ["Check bounds", "Use safe slices", "Validate lengths before unsafe code"],
    "examples": {"vulnerable": "buf.get_unchecked(i)", "secure": "buf.get(i).ok_or(Error)?"}
  },
  {
    "id": "CWE-200",
    "name": "Information Exposure",
    "category": "info",
    "description": "Sensitive data is disclosed to an unauthorized actor.",
    "severity": "Medium",
    "detection_indicators": ["secrets in errors", "debug output", "private fields returned", "stack traces exposed"],
    "remediation_steps": ["Redact sensitive fields", "Return generic errors", "Enforce access checks"],
    "examples": {"vulnerable": "return err.to_string()", "secure": "return public_error_code(err)"}
  },
  {
    "id": "CWE-287",
    "name": "Improper Authentication",
    "category": "auth",
    "description": "Identity is not proven correctly before trust is granted.",
    "severity": "High",
    "detection_indicators": ["trusting user id header", "weak session validation", "missing token verification"],
    "remediation_steps": ["Verify credentials or signed tokens", "Bind sessions to users", "Fail closed"],
    "examples": {"vulnerable": "user = req.headers[\"x-user\"]", "secure": "user = verify_session(req.cookie)"}
  },
  {
    "id": "CWE-306",
    "name": "Missing Authentication",
    "category": "auth",
    "description": "A protected endpoint or action is reachable without authentication.",
    "severity": "High",
    "detection_indicators": ["admin route without auth middleware", "export endpoint public", "mutation before auth"],
    "remediation_steps": ["Require authentication middleware", "Add integration tests", "Centralize route protection"],
    "examples": {"vulnerable": "router.post('/admin/delete', deleteUser)", "secure": "router.post('/admin/delete', requireAuth, deleteUser)"}
  },
  {
    "id": "CWE-327",
    "name": "Broken or Risky Cryptographic Algorithm",
    "category": "crypto",
    "description": "An obsolete or weak cryptographic primitive is used for security.",
    "severity": "Medium",
    "detection_indicators": ["MD5", "SHA1 signatures", "DES", "RC4", "homegrown crypto"],
    "remediation_steps": ["Use modern algorithms", "Use maintained libraries", "Prefer AEAD modes"],
    "examples": {"vulnerable": "md5(password)", "secure": "argon2(password, salt)"}
  },
  {
    "id": "CWE-330",
    "name": "Insufficiently Random Values",
    "category": "crypto",
    "description": "Predictable randomness is used where unpredictability is required.",
    "severity": "Medium",
    "detection_indicators": ["Math.random tokens", "random module tokens", "time-based nonce"],
    "remediation_steps": ["Use CSPRNG APIs", "Generate enough entropy", "Avoid deterministic seeds"],
    "examples": {"vulnerable": "token = random()", "secure": "token = secrets.token_urlsafe(32)"}
  },
  {
    "id": "CWE-347",
    "name": "Improper Verification of Cryptographic Signature",
    "category": "crypto",
    "description": "Signed data is accepted without verifying the signature or algorithm.",
    "severity": "Critical",
    "detection_indicators": ["jwt.decode", "alg none", "signature ignored", "verify=false"],
    "remediation_steps": ["Verify signatures", "Pin algorithms", "Validate issuer and audience"],
    "examples": {"vulnerable": "claims = jwt.decode(token)", "secure": "claims = jwt.verify(token, key, opts)"}
  },
  {
    "id": "CWE-352",
    "name": "Cross-Site Request Forgery",
    "category": "web",
    "description": "A state-changing web action lacks CSRF protection.",
    "severity": "Medium",
    "detection_indicators": ["cookie-auth POST without CSRF", "state mutation via GET", "no SameSite strategy"],
    "remediation_steps": ["Use CSRF tokens", "Set SameSite cookies", "Reject unsafe cross-site requests"],
    "examples": {"vulnerable": "POST /settings without token", "secure": "POST /settings with verified CSRF token"}
  },
  {
    "id": "CWE-362",
    "name": "Race Condition",
    "category": "concurrency",
    "description": "Concurrent execution can violate a security invariant.",
    "severity": "Medium",
    "detection_indicators": ["check then act", "shared mutable state without lock", "non-atomic permission update"],
    "remediation_steps": ["Use transactions", "Lock shared state", "Enforce invariants atomically"],
    "examples": {"vulnerable": "if balance >= amount { withdraw(amount) }", "secure": "withdraw_in_transaction(amount)"}
  },
  {
    "id": "CWE-400",
    "name": "Uncontrolled Resource Consumption",
    "category": "availability",
    "description": "Input can force excessive CPU, memory, file, or network use.",
    "severity": "Medium",
    "detection_indicators": ["unbounded loop over input", "no upload limit", "regex over attacker text", "unbounded concurrency"],
    "remediation_steps": ["Add limits", "Set timeouts", "Bound concurrency and input size"],
    "examples": {"vulnerable": "read_all(upload)", "secure": "read_with_limit(upload, MAX_BYTES)"}
  },
  {
    "id": "CWE-434",
    "name": "Unrestricted File Upload",
    "category": "file",
    "description": "Dangerous uploaded content is accepted or stored unsafely.",
    "severity": "High",
    "detection_indicators": ["trust filename", "no content validation", "store under web root", "no size limit"],
    "remediation_steps": ["Validate type and size", "Generate names", "Store outside executable roots"],
    "examples": {"vulnerable": "save(upload.filename, upload.bytes)", "secure": "save(random_name(), validated_bytes)"}
  },
  {
    "id": "CWE-502",
    "name": "Deserialization of Untrusted Data",
    "category": "deserialization",
    "description": "Untrusted serialized objects can trigger code execution or state corruption.",
    "severity": "Critical",
    "detection_indicators": ["pickle.loads", "eval deserialization", "untrusted object hydrate", "binary formatter"],
    "remediation_steps": ["Use safe formats", "Validate schema", "Avoid object deserialization from users"],
    "examples": {"vulnerable": "pickle.loads(request.data)", "secure": "json.loads(request.data)"}
  },
  {
    "id": "CWE-601",
    "name": "Open Redirect",
    "category": "web",
    "description": "Users can be redirected to attacker-controlled locations.",
    "severity": "Medium",
    "detection_indicators": ["redirect from query param", "Location header from input", "next URL unvalidated"],
    "remediation_steps": ["Allow only relative paths", "Use allowlists", "Map route ids to URLs"],
    "examples": {"vulnerable": "redirect(req.query.next)", "secure": "redirect(allowed_return_path(req.query.next))"}
  },
  {
    "id": "CWE-611",
    "name": "XML External Entity Reference",
    "category": "xml",
    "description": "XML parser resolves external entities from untrusted XML.",
    "severity": "High",
    "detection_indicators": ["resolve_entities true", "DTD enabled", "XML parser network access"],
    "remediation_steps": ["Disable DTDs", "Disable external entities", "Use hardened parsers"],
    "examples": {"vulnerable": "parse_xml(request_body)", "secure": "parse_xml_no_entities(request_body)"}
  },
  {
    "id": "CWE-732",
    "name": "Incorrect Permission Assignment",
    "category": "auth",
    "description": "Files, resources, or roles are created with overly broad permissions.",
    "severity": "Medium",
    "detection_indicators": ["chmod 777", "public bucket", "world-readable secret", "admin role default"],
    "remediation_steps": ["Use least privilege", "Set restrictive defaults", "Test permission boundaries"],
    "examples": {"vulnerable": "chmod(path, 0o777)", "secure": "chmod(path, 0o600)"}
  },
  {
    "id": "CWE-787",
    "name": "Out-of-bounds Write",
    "category": "memory",
    "description": "Code writes outside the intended memory range.",
    "severity": "Critical",
    "detection_indicators": ["unchecked write index", "unsafe copy", "length from input to pointer write"],
    "remediation_steps": ["Check bounds", "Use safe buffers", "Validate lengths before unsafe writes"],
    "examples": {"vulnerable": "ptr.add(i).write(v)", "secure": "slice.get_mut(i).ok_or(Error)? = v"}
  },
  {
    "id": "CWE-798",
    "name": "Hard-coded Credentials",
    "category": "auth",
    "description": "A credential is embedded in code or checked-in configuration.",
    "severity": "High",
    "detection_indicators": ["api key literal", "password literal", "private key block", "token constant"],
    "remediation_steps": ["Move secrets to secret storage", "Rotate exposed credentials", "Scan commits"],
    "examples": {"vulnerable": "const API_KEY = \"sk_live\"", "secure": "const API_KEY = env.API_KEY"}
  },
  {
    "id": "CWE-862",
    "name": "Missing Authorization",
    "category": "auth",
    "description": "A user can perform an action without permission checks.",
    "severity": "High",
    "detection_indicators": ["object access by id only", "delete/update without owner check", "tenant id trusted from input"],
    "remediation_steps": ["Check ownership", "Enforce policy centrally", "Deny by default"],
    "examples": {"vulnerable": "getDocument(req.params.id)", "secure": "getDocumentForUser(user.id, req.params.id)"}
  },
  {
    "id": "CWE-863",
    "name": "Incorrect Authorization",
    "category": "auth",
    "description": "An authorization check exists but uses the wrong subject, object, or policy.",
    "severity": "High",
    "detection_indicators": ["role check without resource check", "client-provided tenant", "confused deputy"],
    "remediation_steps": ["Use object-level policies", "Bind tenant to session", "Test negative cases"],
    "examples": {"vulnerable": "if user.role == 'editor' { edit(doc) }", "secure": "policy.can_edit(user, doc)"}
  },
  {
    "id": "CWE-918",
    "name": "Server-Side Request Forgery",
    "category": "network",
    "description": "A server can be induced to make requests to attacker-chosen locations.",
    "severity": "High",
    "detection_indicators": ["request URL from user", "webhook fetch without allowlist", "metadata IP reachable"],
    "remediation_steps": ["Use allowlists", "Block private networks", "Resolve targets from trusted IDs"],
    "examples": {"vulnerable": "fetch(req.query.url)", "secure": "fetch(allowlistedEndpoint(req.query.id))"}
  },
  {
    "id": "CWE-943",
    "name": "Improper Neutralization in Data Query Logic",
    "category": "injection",
    "description": "Untrusted data changes the meaning of a non-SQL query or filter.",
    "severity": "High",
    "detection_indicators": ["NoSQL operator injection", "LDAP filter concatenation", "XPath concatenation"],
    "remediation_steps": ["Use typed query APIs", "Reject operator keys", "Escape query syntax"],
    "examples": {"vulnerable": "find(JSON.parse(req.body.filter))", "secure": "find({id: validated_id})"}
  },
  {
    "id": "CWE-1004",
    "name": "Sensitive Cookie Without HttpOnly",
    "category": "web",
    "description": "Sensitive cookies are accessible to client-side scripts.",
    "severity": "Medium",
    "detection_indicators": ["session cookie no HttpOnly", "auth cookie readable by JS", "missing secure cookie flags"],
    "remediation_steps": ["Set HttpOnly", "Set Secure", "Set SameSite appropriately"],
    "examples": {"vulnerable": "Set-Cookie: sid=abc", "secure": "Set-Cookie: sid=abc; HttpOnly; Secure; SameSite=Lax"}
  },
  {
    "id": "CWE-1321",
    "name": "Prototype Pollution",
    "category": "object",
    "description": "Attacker-controlled object keys modify object prototypes.",
    "severity": "High",
    "detection_indicators": ["recursive merge from input", "__proto__ key accepted", "constructor.prototype path"],
    "remediation_steps": ["Reject prototype keys", "Use null-prototype objects", "Use safe merge utilities"],
    "examples": {"vulnerable": "merge({}, req.body)", "secure": "mergeSafe({}, stripPrototypeKeys(req.body))"}
  }
]
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::HashSet;

    #[test]
    fn sast_rules_have_expected_coverage() {
        let rules: Vec<Value> = serde_json::from_str(SAST_RULES).unwrap();
        let categories: HashSet<String> = rules
            .iter()
            .filter_map(|rule| rule["category"].as_str().map(str::to_string))
            .collect();

        assert!(rules.len() >= 20);
        assert!(categories.len() >= 15);
        assert!(rules.iter().all(|rule| rule["bad_example"].is_string()));
        assert!(rules.iter().all(|rule| rule["good_example"].is_string()));
    }

    #[test]
    fn cwe_entries_have_expected_coverage() {
        let entries: Vec<Value> = serde_json::from_str(CWE_ENTRIES).unwrap();

        assert!(entries.len() >= 25);
        assert!(entries.iter().all(|entry| entry["detection_indicators"]
            .as_array()
            .is_some_and(|items| !items.is_empty())));
        assert!(entries.iter().all(|entry| entry["remediation_steps"]
            .as_array()
            .is_some_and(|items| !items.is_empty())));
        assert!(entries.iter().any(|entry| entry["id"] == "CWE-89"));
        assert!(entries.iter().any(|entry| entry["id"] == "CWE-918"));
    }
}
