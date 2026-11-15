import { describe, expect, it } from "vitest";
import { policyGlobToRegExp } from "./policyGlob";

describe("policyGlobToRegExp", () => {
  it("matches `*` as a wildcard", () => {
    expect(policyGlobToRegExp("rm -rf *").test("rm -rf dist")).toBe(true);
    expect(policyGlobToRegExp("rm -rf *").test("git status")).toBe(false);
    expect(policyGlobToRegExp("git push *--force*").test("git push origin main --force")).toBe(
      true
    );
  });

  it("treats every other regex character as literal", () => {
    expect(() => policyGlobToRegExp("rm [")).not.toThrow();
    expect(policyGlobToRegExp("rm [").test("rm [")).toBe(true);
    expect(policyGlobToRegExp("git commit (wip").test("git commit (wip)")).toBe(false);
    expect(policyGlobToRegExp("file.txt").test("fileXtxt")).toBe(false);
  });
});
