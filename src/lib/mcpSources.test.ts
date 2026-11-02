import { describe, expect, it } from "vitest";
import {
  applySourceText,
  emptyDraft,
  entryFromJson,
  isLikelySecretKey,
  requestFromDraft,
  splitCommandLine,
  suggestName,
  validationErrors,
} from "./mcpSources";

describe("splitCommandLine", () => {
  it("splits on whitespace and keeps quoted groups", () => {
    expect(splitCommandLine('npx -y @scope/server --arg "a b"')).toEqual([
      "npx",
      "-y",
      "@scope/server",
      "--arg",
      "a b",
    ]);
    expect(splitCommandLine("cmd 'x y'")).toEqual(["cmd", "x y"]);
    expect(splitCommandLine("")).toEqual([]);
  });
});

describe("applySourceText", () => {
  it("detects a pasted URL as http and suggests a hostname name", () => {
    const next = applySourceText(emptyDraft(), "https://api.mobbin.com/mcp");
    expect(next.transport).toBe("http");
    expect(next.url).toBe("https://api.mobbin.com/mcp");
    expect(next.displayName).toBe("api.mobbin.com");
  });

  it("detects a pasted command line as stdio", () => {
    const next = applySourceText(emptyDraft(), "npx -y @modelcontextprotocol/server-everything");
    expect(next.transport).toBe("stdio");
    expect(next.commandLine).toBe("npx -y @modelcontextprotocol/server-everything");
    expect(next.displayName).toBe("server-everything");
  });

  it("fills fields from a JSON config with a remote entry", () => {
    const json = JSON.stringify({
      mcpServers: {
        mobbin: { type: "remote", url: "https://api.mobbin.com/mcp" },
      },
    });
    const next = applySourceText(emptyDraft(), json);
    expect(next.transport).toBe("http");
    expect(next.url).toBe("https://api.mobbin.com/mcp");
    expect(next.displayName).toBe("mobbin");
  });

  it("fills command args from an OpenCode local entry and marks secret env", () => {
    const json = JSON.stringify({
      mcp: {
        everything: {
          type: "local",
          command: ["npx", "-y", "server-everything"],
          environment: { GITHUB_TOKEN: "tok", LOG: "1" },
        },
      },
    });
    const next = applySourceText(emptyDraft(), json);
    expect(next.transport).toBe("stdio");
    expect(next.commandLine).toBe("npx -y server-everything");
    expect(next.displayName).toBe("everything");
    const token = next.env.find((entry) => entry.key === "GITHUB_TOKEN");
    const log = next.env.find((entry) => entry.key === "LOG");
    expect(token?.secret).toBe(true);
    expect(log?.secret).toBe(false);
  });

  it("keeps editing while JSON is incomplete", () => {
    const draft = emptyDraft();
    const next = applySourceText(draft, '{"mcpServers": {');
    expect(next.transport).toBe("stdio");
    expect(next.sourceText).toBe('{"mcpServers": {');
  });
});

describe("entryFromJson", () => {
  it("finds entries in every supported map shape", () => {
    const remote = { type: "remote", url: "https://x.test/mcp" };
    for (const doc of [
      { mcpServers: { srv: remote } },
      { servers: { srv: remote } },
      { mcp: { servers: { srv: remote } } },
      { mcp: { mcpServers: { srv: remote } } },
      { mcp: { srv: remote } },
    ]) {
      const found = entryFromJson(doc);
      expect(found?.name).toBe("srv");
      expect(found?.entry.url).toBe("https://x.test/mcp");
    }
  });

  it("treats a bare server object as the entry", () => {
    const found = entryFromJson({ command: "node", args: ["s.js"] });
    expect(found?.entry.transport).toBe("stdio");
    expect(found?.entry.command).toBe("node");
  });
});

describe("requestFromDraft", () => {
  it("splits a stdio command line into command + args and splits secret env", () => {
    const draft = applySourceText(emptyDraft(), "npx -y pkg --flag 'a b'");
    draft.env = [
      { id: "1", key: "LOG", value: "1", secret: false },
      { id: "2", key: "TOKEN", value: "abc", secret: true },
    ];
    const request = requestFromDraft(draft);
    expect(request.command).toBe("npx");
    expect(request.args).toEqual(["-y", "pkg", "--flag", "a b"]);
    expect(request.env).toEqual([{ key: "LOG", value: "1" }]);
    expect(request.secretEnv).toEqual([{ key: "TOKEN", value: "abc" }]);
  });

  it("maps http drafts to url + header split", () => {
    const draft = applySourceText(emptyDraft(), "https://api.mobbin.com/mcp");
    draft.headers = [
      { id: "1", key: "X-Team", value: "core", secret: false },
      { id: "2", key: "Authorization", value: "Bearer t", secret: true },
    ];
    const request = requestFromDraft(draft);
    expect(request.transport).toBe("http");
    expect(request.url).toBe("https://api.mobbin.com/mcp");
    expect(request.headers).toEqual([{ key: "X-Team", value: "core" }]);
    expect(request.secretHeaders).toEqual([{ key: "Authorization", value: "Bearer t" }]);
  });
});

describe("validationErrors", () => {
  it("requires a name and a command for stdio", () => {
    const errors = validationErrors(emptyDraft());
    expect(errors).toContain("Name is required.");
    expect(errors).toContain("Command is required.");
  });

  it("requires an https URL for http", () => {
    const draft = { ...emptyDraft(), transport: "http" as const, displayName: "x" };
    expect(validationErrors(draft)).toContain("URL is required.");
    const bad = { ...draft, url: "ftp://x" };
    expect(validationErrors(bad)).toContain("URL must start with https:// or http://.");
  });

  it("rejects secret/non-secret overlap on headers", () => {
    const draft = {
      ...emptyDraft(),
      transport: "http" as const,
      displayName: "x",
      url: "https://x.test/mcp",
      headers: [
        { id: "1", key: "K", value: "a", secret: false },
        { id: "2", key: "K", value: "b", secret: true },
      ],
    };
    expect(validationErrors(draft).some((e) => e.includes("both secret"))).toBe(true);
  });
});

describe("suggestName", () => {
  it("strips npm scope and non-name characters", () => {
    expect(suggestName("stdio", "", "npx -y @modelcontextprotocol/server-everything")).toBe(
      "server-everything"
    );
    expect(suggestName("http", "https://www.example.com/mcp", "")).toBe("example.com");
  });
});

describe("isLikelySecretKey", () => {
  it("flags authorization and token-ish keys", () => {
    expect(isLikelySecretKey("Authorization")).toBe(true);
    expect(isLikelySecretKey("GITHUB_TOKEN")).toBe(true);
    expect(isLikelySecretKey("LOG_LEVEL")).toBe(false);
  });
});
