export function levenshtein(a: string, b: string): number {
  const m = a.length;
  const n = b.length;

  if (m === 0) return n;
  if (n === 0) return m;

  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));

  for (let i = 0; i <= m; i++) dp[i][0] = i;
  for (let j = 0; j <= n; j++) dp[0][j] = j;

  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      dp[i][j] = Math.min(dp[i - 1][j] + 1, dp[i][j - 1] + 1, dp[i - 1][j - 1] + cost);
    }
  }

  return dp[m][n];
}

export function findClosestSkill(
  query: string,
  skills: { name: string }[],
  maxDistance = 3
): string | null {
  const lower = query.toLowerCase();
  let bestName = "";
  let bestDist = Infinity;
  for (const s of skills) {
    const dist = levenshtein(lower, s.name.toLowerCase());
    if (dist < bestDist) {
      bestDist = dist;
      bestName = s.name;
    }
  }
  return bestDist <= maxDistance && bestName ? bestName : null;
}
