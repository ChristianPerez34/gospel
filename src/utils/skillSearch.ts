import { findClosestSkill } from "./levenshtein";

export { findClosestSkill };

export interface SkillLike {
  name: string;
  description?: string;
  source?: string;
  user_invocable?: boolean;
}

export interface FilterSkillsResult<T> {
  filtered: T[];
  suggestion: string | null;
}

/**
 * Calculates the Jaccard similarity coefficient between two sets:
 * J(A, B) = |A ∩ B| / |A ∪ B|
 * Returns a value between 0 and 1.
 */
export function jaccardSimilarity<T>(setA: Set<T>, setB: Set<T>): number {
  if (setA.size === 0 && setB.size === 0) return 0;
  let intersection = 0;
  for (const item of setA) {
    if (setB.has(item)) {
      intersection++;
    }
  }
  const union = setA.size + setB.size - intersection;
  return union === 0 ? 0 : intersection / union;
}

/**
 * Extracts character n-grams from a string.
 */
export function getNgrams(str: string, n = 2): Set<string> {
  const ngrams = new Set<string>();
  const clean = str.trim().toLowerCase();
  if (clean.length < n) {
    if (clean.length > 0) ngrams.add(clean);
    return ngrams;
  }
  for (let i = 0; i <= clean.length - n; i++) {
    ngrams.add(clean.slice(i, i + n));
  }
  return ngrams;
}

/**
 * Calculates character n-gram Jaccard similarity between two strings.
 */
export function ngramJaccardSimilarity(a: string, b: string, n = 2): number {
  return jaccardSimilarity(getNgrams(a, n), getNgrams(b, n));
}

/**
 * Tokenizes a string on non-alphanumeric characters (hyphens, underscores, whitespace, etc.).
 */
export function tokenize(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((token) => token.length > 0);
}

/**
 * Calculates word-token Jaccard similarity between two strings.
 */
export function tokenJaccardSimilarity(a: string, b: string): number {
  return jaccardSimilarity(new Set(tokenize(a)), new Set(tokenize(b)));
}

/**
 * Scores how well a skill matches a query. Returns a positive score for matches,
 * or 0 if the skill does not match.
 */
export function scoreSkill(query: string, skill: SkillLike): number {
  const q = query.trim().toLowerCase();
  if (!q) return 0;

  const name = skill.name.toLowerCase();
  const desc = (skill.description || "").toLowerCase();

  const qNormalized = q.replace(/[^a-z0-9]/g, "");
  const nameNormalized = name.replace(/[^a-z0-9]/g, "");

  // 1. Exact match on name
  if (name === q) {
    return 10000;
  }

  // 2. Normalized exact match (e.g. "codereview" matches "code-review")
  if (qNormalized.length > 0 && nameNormalized === qNormalized) {
    return 9500;
  }

  // 3. Name starts with query (prefix match)
  if (name.startsWith(q)) {
    return 8000 + Math.round(500 * (q.length / name.length));
  }

  // 4. Normalized name starts with normalized query
  if (qNormalized.length > 1 && nameNormalized.startsWith(qNormalized)) {
    return 7500 + Math.round(500 * (qNormalized.length / nameNormalized.length));
  }

  // 5. Name segment starts with or equals query (e.g. "code-review" matches "review")
  const segments = name.split(/[^a-z0-9]+/).filter(Boolean);
  let segmentScore = 0;
  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i];
    if (seg === q) {
      segmentScore = Math.max(segmentScore, 7000 - i * 100);
    } else if (seg.startsWith(q)) {
      segmentScore = Math.max(
        segmentScore,
        6000 + Math.round(400 * (q.length / seg.length)) - i * 100
      );
    }
  }
  if (segmentScore > 0) {
    const tokenJaccard = tokenJaccardSimilarity(q, name);
    const ngramJaccard = ngramJaccardSimilarity(q, name, 2);
    return segmentScore + Math.round(tokenJaccard * 300) + Math.round(ngramJaccard * 300);
  }

  // 6. Name contains query as substring
  if (name.includes(q)) {
    const tokenJaccard = tokenJaccardSimilarity(q, name);
    const ngramJaccard = ngramJaccardSimilarity(q, name, 2);
    return 5000 + Math.round(tokenJaccard * 300) + Math.round(ngramJaccard * 300);
  }
  if (qNormalized.length > 1 && nameNormalized.includes(qNormalized)) {
    return 4500;
  }

  // 7. Full-text search across query tokens in name and description
  const queryTokens = tokenize(q);
  if (queryTokens.length > 0) {
    const nameTokens = segments;
    const descTokens = tokenize(desc);
    const allTokens = [...nameTokens, ...descTokens];

    const allInName = queryTokens.every((qt) =>
      nameTokens.some((nt) => nt.includes(qt) || nt.startsWith(qt))
    );
    if (allInName) {
      return 4000 + Math.round(tokenJaccardSimilarity(q, name) * 500);
    }

    const allInText = queryTokens.every((qt) =>
      allTokens.some((t) => t.includes(qt) || t.startsWith(qt))
    );
    if (allInText) {
      return 3000 + Math.round(tokenJaccardSimilarity(q, `${name} ${desc}`) * 500);
    }

    if (desc.includes(q)) {
      return 2500;
    }
  }

  return 0;
}

/**
 * Filters and ranks skills against a user query using full-text search, token matching,
 * substring matching, and Jaccard similarity.
 */
export function filterSkills<T extends SkillLike>(
  skills: T[],
  query: string
): FilterSkillsResult<T> {
  const trimmed = query.trim();
  if (!trimmed) {
    return { filtered: skills, suggestion: null };
  }

  const scored: Array<{ skill: T; score: number }> = [];
  for (const skill of skills) {
    const score = scoreSkill(trimmed, skill);
    if (score > 0) {
      scored.push({ skill, score });
    }
  }

  if (scored.length > 0) {
    scored.sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score;
      return a.skill.name.localeCompare(b.skill.name);
    });
    return { filtered: scored.map((item) => item.skill), suggestion: null };
  }

  return { filtered: [], suggestion: findClosestSkill(trimmed, skills) };
}
