// Gospel approval rules use `*` as a wildcard; every other character is literal.
export function policyGlobToRegExp(pattern: string): RegExp {
  const literal = pattern
    .split("*")
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join(".*");
  return new RegExp(`^${literal}$`, "i");
}
