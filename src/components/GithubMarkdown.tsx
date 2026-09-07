//! GitHub-flavoured Markdown for pull request descriptions and discussion.
//!
//! PR bodies are documents rather than streaming chat, so they need the full
//! set of structures people use on GitHub: headings, links, images, tables,
//! task lists and strikethrough as well as ordinary CommonMark. `react-markdown`
//! builds React nodes (it never injects the source as HTML), and its default
//! URL transform rejects unsafe protocols such as `javascript:`.

import ReactMarkdown, {
  defaultUrlTransform,
  type UrlTransform,
} from "react-markdown";
import remarkGfm from "remark-gfm";

export function GithubMarkdown({
  children,
  repository,
  revision,
  sourceUrl,
}: {
  children: string;
  repository?: string;
  revision?: string;
  sourceUrl?: string;
}) {
  let origin = "https://github.com";
  try {
    if (sourceUrl) origin = new URL(sourceUrl).origin;
  } catch {
    // A malformed source URL must not stop the body itself from rendering.
  }

  const transform: UrlTransform = (url, key, node) => {
    void node;
    const safe = defaultUrlTransform(url);
    if (!safe || !repository || !revision) return safe;
    if (safe.startsWith("#")) return sourceUrl ? `${sourceUrl}${safe}` : safe;
    if (safe.startsWith("//")) return `https:${safe}`;
    if (safe.startsWith("/")) return `${origin}${safe}`;
    if (/^[a-z][a-z\d+.-]*:/i.test(safe)) return safe;

    const base =
      key === "src"
        ? `${origin}/${repository}/raw/${revision}/`
        : `${origin}/${repository}/blob/${revision}/`;
    return new URL(safe, base).href;
  };

  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      urlTransform={transform}
      components={{
        a: ({ node, children: label, ...props }) => {
          void node;
          if (!props.href) return label;
          return (
            <a {...props} target="_blank" rel="noreferrer">
              {label}
            </a>
          );
        },
        img: ({ node, ...props }) => {
          void node;
          if (!props.src) return <span>{props.alt}</span>;
          return <img {...props} loading="lazy" />;
        },
      }}
    >
      {children}
    </ReactMarkdown>
  );
}
