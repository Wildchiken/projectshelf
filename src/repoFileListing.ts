export type FileListEntry =
  | { kind: "file"; name: string; path: string }
  | { kind: "dir"; name: string; prefix: string };

export function listEntriesAtPrefix(
  blobPaths: string[],
  prefix: string,
): FileListEntry[] {
  const norm = prefix.replace(/\/$/, "");
  const pfx = norm ? `${norm}/` : "";
  const seen = new Map<string, string[]>();
  for (const p of blobPaths) {
    if (!p.startsWith(pfx)) continue;
    const rest = p.slice(pfx.length);
    if (!rest) continue;
    const slash = rest.indexOf("/");
    const head = slash === -1 ? rest : rest.slice(0, slash);
    if (!seen.has(head)) seen.set(head, []);
    seen.get(head)!.push(rest);
  }
  const entries: FileListEntry[] = [];
  for (const [head, rels] of seen) {
    const isFile = rels.length === 1 && rels[0] === head;
    if (isFile) {
      entries.push({ kind: "file", name: head, path: pfx + head });
    } else {
      entries.push({ kind: "dir", name: head, prefix: pfx + head });
    }
  }
  entries.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === "dir" ? -1 : 1;
    return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  });
  return entries;
}

const LICENSE_BASE =
  /^(LICENSE|LICENCE|COPYING|COPYRIGHT)(\.md|\.txt|\.markdown)?$/i;

const GO_TO_FILE_MAX = 50;

export function rankPathsForGoToFile(query: string, paths: string[]): string[] {
  const q = query.trim();
  if (!q) return [];
  const scored: { path: string; score: number }[] = [];
  for (const path of paths) {
    const s = fuzzyPathScore(path, q);
    if (s !== null) scored.push({ path, score: s });
  }
  scored.sort((a, b) => {
    if (a.score !== b.score) return a.score - b.score;
    return a.path.length - b.path.length;
  });
  return scored.slice(0, GO_TO_FILE_MAX).map((x) => x.path);
}

function fuzzyPathScore(path: string, query: string): number | null {
  const p = path.toLowerCase();
  const q = query.toLowerCase();
  let qi = 0;
  let score = 0;
  let prev = -1;
  for (let i = 0; i < p.length && qi < q.length; i++) {
    if (p[i] === q[qi]) {
      const gap = prev < 0 ? 0 : i - prev - 1;
      score += gap * 2;
      const atSeg = i === 0 || p[i - 1] === "/";
      if (atSeg) score -= 3;
      prev = i;
      qi++;
    }
  }
  if (qi < q.length) return null;
  const slash = path.lastIndexOf("/");
  const base = slash >= 0 ? path.slice(slash + 1) : path;
  if (base.toLowerCase().startsWith(q)) score -= 5;
  score += path.length * 0.01;
  return score;
}

export function findLicensePath(paths: string[]): string | null {
  for (const p of paths) {
    const base = p.includes("/") ? p.slice(p.lastIndexOf("/") + 1) : p;
    if (LICENSE_BASE.test(base)) return p;
  }
  return null;
}

export function formatRelativeTime(
  dateUnix: number,
  locale: "zh-CN" | "en-US" = "zh-CN",
): string {
  const deltaSec = Math.max(0, Math.floor(Date.now() / 1000 - dateUnix));
  if (locale === "en-US") {
    const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
    if (deltaSec < 60) return rtf.format(-deltaSec, "second");
    const min = Math.floor(deltaSec / 60);
    if (min < 60) return rtf.format(-min, "minute");
    const h = Math.floor(min / 60);
    if (h < 24) return rtf.format(-h, "hour");
    const d = Math.floor(h / 24);
    if (d < 7) return rtf.format(-d, "day");
    return new Date(dateUnix * 1000).toLocaleDateString("en");
  }
  if (deltaSec < 45) return "刚刚";
  if (deltaSec < 3600) return `${Math.floor(deltaSec / 60)} 分钟前`;
  if (deltaSec < 86400) return `${Math.floor(deltaSec / 3600)} 小时前`;
  if (deltaSec < 86400 * 7) return `${Math.floor(deltaSec / 86400)} 天前`;
  return new Date(dateUnix * 1000).toLocaleDateString("zh-CN");
}
