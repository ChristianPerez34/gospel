// Gospel approval rules use `*` as a wildcard; every other character is literal.
export function policyGlobToRegExp(pattern: string): RegExp {
  const escapeLiteral = (part: string) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

  if (!pattern.includes("*")) {
    return new RegExp(`^${escapeLiteral(pattern)}(?:\\s.*)?$`, "i");
  }

  if (pattern.endsWith(" *") && pattern.slice(0, -2).trim().length > 0) {
    const prefix = pattern.slice(0, -2).split("*").map(escapeLiteral).join(".*");
    return new RegExp(`^${prefix}(?:\\s.*)?$`, "i");
  }

  const literal = pattern.split("*").map(escapeLiteral).join(".*");
  return new RegExp(`^${literal}$`, "i");
}
