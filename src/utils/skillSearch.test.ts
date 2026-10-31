import { describe, expect, it } from "vitest";
import {
  filterSkills,
  findClosestSkill,
  getNgrams,
  jaccardSimilarity,
  ngramJaccardSimilarity,
  scoreSkill,
  tokenize,
  tokenJaccardSimilarity,
} from "./skillSearch";

describe("skillSearch", () => {
  const sampleSkills = [
    { name: "code-review", description: "Review pull requests and code changes" },
    { name: "review-animations", description: "Audit and polish UI animations" },
    { name: "codebase-design", description: "Shared vocabulary for deep modules" },
    { name: "diagnosing-bugs", description: "Diagnosis loop for hard bugs" },
    { name: "tdd", description: "Test-driven development workflow" },
    { name: "handoff", description: "Prepare session summary and handover notes" },
  ];

  describe("jaccardSimilarity and helpers", () => {
    it("computes Jaccard similarity between two sets", () => {
      const setA = new Set(["a", "b", "c"]);
      const setB = new Set(["b", "c", "d"]);
      // intersection: {b, c} (2), union: {a, b, c, d} (4)
      expect(jaccardSimilarity(setA, setB)).toBe(0.5);
    });

    it("handles empty sets", () => {
      expect(jaccardSimilarity(new Set(), new Set())).toBe(0);
      expect(jaccardSimilarity(new Set(["a"]), new Set())).toBe(0);
    });

    it("tokenizes text on punctuation and whitespace", () => {
      expect(tokenize("code-review")).toEqual(["code", "review"]);
      expect(tokenize("review_animations")).toEqual(["review", "animations"]);
      expect(tokenize("Multiple   words! and-hyphens")).toEqual([
        "multiple",
        "words",
        "and",
        "hyphens",
      ]);
    });

    it("computes token Jaccard similarity", () => {
      expect(tokenJaccardSimilarity("review", "code-review")).toBe(0.5);
      expect(tokenJaccardSimilarity("review", "review-animations")).toBe(0.5);
      expect(tokenJaccardSimilarity("review", "tdd")).toBe(0);
    });

    it("extracts character n-grams and computes ngram Jaccard similarity", () => {
      const ngrams = getNgrams("code", 2);
      expect(Array.from(ngrams).sort()).toEqual(["co", "de", "od"]);

      const similarity = ngramJaccardSimilarity("review", "code-review", 2);
      expect(similarity).toBeGreaterThan(0.3);
      expect(ngramJaccardSimilarity("review", "tdd", 2)).toBe(0);
    });

    it("scores matching skills higher than non-matches", () => {
      const exactScore = scoreSkill("code-review", { name: "code-review" });
      const prefixScore = scoreSkill("review", { name: "review-animations" });
      const segmentScore = scoreSkill("review", { name: "code-review" });
      const nonMatchScore = scoreSkill("review", { name: "tdd" });

      expect(exactScore).toBeGreaterThan(prefixScore);
      expect(prefixScore).toBeGreaterThan(nonMatchScore);
      expect(segmentScore).toBeGreaterThan(nonMatchScore);
      expect(nonMatchScore).toBe(0);
    });
  });

  describe("filterSkills", () => {
    it("returns all skills when query is empty", () => {
      const result = filterSkills(sampleSkills, "");
      expect(result.filtered).toHaveLength(sampleSkills.length);
      expect(result.suggestion).toBeNull();
    });

    it("returns both code-review and review-animations when typing /review", () => {
      const result = filterSkills(sampleSkills, "review");
      const names = result.filtered.map((s) => s.name);

      expect(names).toContain("code-review");
      expect(names).toContain("review-animations");
      expect(names).not.toContain("tdd");
      expect(names).not.toContain("diagnosing-bugs");
      expect(result.suggestion).toBeNull();
    });

    it("matches skills by prefix and segment prefix", () => {
      const resultAnim = filterSkills(sampleSkills, "anim");
      expect(resultAnim.filtered.map((s) => s.name)).toContain("review-animations");

      const resultDesign = filterSkills(sampleSkills, "design");
      expect(resultDesign.filtered.map((s) => s.name)).toContain("codebase-design");

      const resultCode = filterSkills(sampleSkills, "code");
      const codeNames = resultCode.filtered.map((s) => s.name);
      expect(codeNames).toContain("code-review");
      expect(codeNames).toContain("codebase-design");
    });

    it("is case-insensitive", () => {
      const result = filterSkills(sampleSkills, "REVIEW");
      const names = result.filtered.map((s) => s.name);
      expect(names).toContain("code-review");
      expect(names).toContain("review-animations");
    });

    it("matches full-text keywords in description", () => {
      const result = filterSkills(sampleSkills, "handover");
      expect(result.filtered.map((s) => s.name)).toContain("handoff");
    });

    it("matches normalized query without hyphens", () => {
      const result = filterSkills(sampleSkills, "codereview");
      expect(result.filtered.map((s) => s.name)).toContain("code-review");
    });

    it("returns suggestion when no skills match", () => {
      const result = filterSkills(sampleSkills, "handofx");
      expect(result.filtered).toEqual([]);
      expect(result.suggestion).toBe("handoff");
    });
  });

  describe("findClosestSkill", () => {
    it("suggests the closest skill on typo in full name", () => {
      expect(findClosestSkill("handof", sampleSkills)).toBe("handoff");
    });

    it("suggests closest skill on typo in segment name", () => {
      expect(findClosestSkill("animatons", sampleSkills)).toBe("review-animations");
    });

    it("returns null for completely unrelated gibberish", () => {
      expect(findClosestSkill("xyz123abc", sampleSkills)).toBeNull();
    });
  });
});
