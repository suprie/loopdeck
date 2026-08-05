import { useState, useCallback, type ComponentPropsWithoutRef } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Check, Copy } from "lucide-react";

/**
 * Markdown renderer for chat / decisions / loops content.
 *
 * The agent's replies, decision records, and loop notes are authored as
 * markdown — without a renderer they collapse into a single whitespace-pre
 * paragraph and tables, lists, headings, and code blocks all become flat
 * text. This component pipes that content through `react-markdown` +
 * `remark-gfm` (tables, strikethrough, task lists) + `rehype-highlight`
 * (syntax highlighting), themed to match the Selasar token system so it
 * reads identically in light and dark mode.
 *
 * Custom component overrides:
 *  - `code` is split into inline spans vs fenced blocks. Fenced blocks get a
 *    header bar showing the language and a copy button.
 *  - `a` opens external links in the user's browser (Tauri's shell plugin)
 *    rather than navigating the memory-history router, which would 404.
 */
export function Markdown({ children }: { children: string }) {
  return (
    <div className="text-sm leading-relaxed break-words">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[[rehypeHighlight, { detect: true, ignoreMissing: true }]]}
        components={{
          // Headings — tight, dev-tool feel.
          h1: (props) => <h1 className="text-base font-semibold tracking-tight mt-4 mb-2 first:mt-0" {...props} />,
          h2: (props) => <h2 className="text-sm font-semibold tracking-tight mt-4 mb-2 first:mt-0" {...props} />,
          h3: (props) => <h3 className="text-sm font-semibold mt-3 mb-1.5 first:mt-0" {...props} />,
          h4: (props) => <h4 className="text-sm font-medium mt-3 mb-1 first:mt-0" {...props} />,
          h5: (props) => <h5 className="text-xs font-semibold uppercase tracking-wider mt-3 mb-1 first:mt-0" {...props} />,
          h6: (props) => <h6 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mt-3 mb-1 first:mt-0" {...props} />,

          // Paragraphs and text.
          p: (props) => <p className="my-2 first:mt-0 last:mb-0" {...props} />,
          strong: (props) => <strong className="font-semibold text-foreground" {...props} />,
          em: (props) => <em className="italic" {...props} />,
          del: (props) => <del className="text-muted-foreground line-through" {...props} />,

          // Links — external http(s)/mailto only. Anything else (file://,
          // javascript:, custom schemes) is suppressed: the CSP blocks
          // javascript:, and we drop the href entirely so the rendered anchor
          // becomes inert rather than handing an unknown scheme to the OS via
          // Tauri's shell `open`.
          a: ({ children, href, ...rest }) => {
            const safe =
              typeof href === "string" &&
              /^(https?:|mailto:)/i.test(href);
            return (
              <a
                href={safe ? href : undefined}
                title={safe ? href : undefined}
                onClick={(e) => {
                  if (!safe) {
                    e.preventDefault();
                    return;
                  }
                  // Defer the dynamic import so it doesn't dominate first paint.
                  e.preventDefault();
                  void import("@tauri-apps/plugin-shell")
                    .then((m) => m.open(href as string))
                    .catch(() => {
                      // Fallback: open in whatever the platform allows.
                      window.open(href, "_blank", "noopener,noreferrer");
                    });
                }}
                className="text-primary underline decoration-primary/30 underline-offset-2 hover:decoration-primary/70 transition-colors"
                {...rest}
              >
                {children}
              </a>
            );
          },

          // Lists.
          ul: (props) => <ul className="my-2 ml-5 list-disc space-y-1 marker:text-muted-foreground" {...props} />,
          ol: (props) => <ol className="my-2 ml-5 list-decimal space-y-1 marker:text-muted-foreground" {...props} />,
          li: (props) => <li className="pl-1" {...props} />,

          // Task list — GFM turns `[]` items into <li class="task-list-item">.
          // We strip the native checkbox styling and render the marker ourselves
          // so it inherits the muted/foreground token colors.
          input: ({ type, checked, ...rest }) =>
            type === "checkbox" ? (
              <span
                className={`inline-block size-3 mr-1.5 align-text-bottom rounded-[3px] border ${
                  checked
                    ? "bg-primary border-primary text-primary-foreground"
                    : "border-border bg-transparent"
                }`}
                aria-checked={checked}
              >
                {checked && <Check className="size-3" strokeWidth={3} />}
              </span>
            ) : (
              <input type={type} checked={checked} {...rest} />
            ),

          // Blockquote.
          blockquote: (props) => (
            <blockquote
              className="my-2 pl-3 border-l-2 border-primary/40 text-muted-foreground italic"
              {...props}
            />
          ),

          // Horizontal rule.
          hr: () => <hr className="my-3 border-border" />,

          // Tables — GFM tables. Wrap in a scroll container so wide tables
          // don't blow out the bubble width.
          table: (props) => (
            <div className="my-3 overflow-x-auto rounded-md border border-border">
              <table className="w-full text-xs border-collapse" {...props} />
            </div>
          ),
          thead: (props) => <thead className="bg-muted/50" {...props} />,
          th: (props) => (
            <th
              className="text-left font-semibold px-2.5 py-1.5 border-b border-border text-foreground"
              {...props}
            />
          ),
          td: (props) => <td className="px-2.5 py-1.5 border-b border-border/60 align-top" {...props} />,

          // Code — distinguish inline from fenced.
          code: CodeBlock,

          // Pre — strip react-markdown's default <pre> since CodeBlock
          // provides its own container; otherwise we'd get nested scrollbars.
          pre: (props) => <>{props.children}</>,
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}

// ── Code handling ────────────────────────────────────────────────────────────

/**
 * Distinguish inline code from fenced code blocks and render each accordingly.
 *
 * react-markdown passes the inline flag via `inline` on the props (the children
 * of a `<pre>` are the fenced block; a bare `code` node is inline). We rely on
 * the presence of a `\n` in the text content as a robust fallback for the
 * common case where the markdown source contains a fenced block.
 */
function CodeBlock({
  inline,
  className,
  children,
  ...rest
}: ComponentPropsWithoutRef<"code"> & { inline?: boolean }) {
  const content = String(children ?? "");
  const isBlock = !inline && (content.includes("\n") || /language-/.test(className ?? ""));

  if (!isBlock) {
    return (
      <code
        className="font-mono text-[0.85em] rounded bg-muted/70 px-1.5 py-0.5 text-foreground"
        {...rest}
      >
        {children}
      </code>
    );
  }

  // Fenced block — header with language tag + copy button, scrollable body.
  // rehype-highlight has already tokenised `children` into <span>s with
  // hljs-* classes; we render them verbatim inside our shell's <pre><code>.
  const lang = /language-(\w+)/.exec(className ?? "")?.[1];
  return (
    <CodeShell lang={lang} content={content}>
      <code className={className} {...rest}>
        {children}
      </code>
    </CodeShell>
  );
}

/**
 * Fenced-code shell: a header bar (language label + copy button) wrapping the
 * highlighted <pre><code> that rehype-highlight produced.
 *
 * Mirrors the look of GitHub / Linear code blocks — subtle border, mono font,
 * a copy affordance that swaps to a check for 1.5s on success.
 */
function CodeShell({
  lang,
  content,
  children,
}: {
  lang?: string;
  content: string;
  children: React.ReactNode;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(content.replace(/\n$/, "")).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }).catch(() => {
      // Clipboard can fail in non-secure contexts; silently ignore.
    });
  }, [content]);

  return (
    <div className="my-3 rounded-md border border-border bg-muted/40 overflow-hidden first:mt-0">
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-border bg-muted/60">
        <span className="text-[10px] uppercase tracking-wider text-muted-foreground font-mono">
          {lang ?? "text"}
        </span>
        <button
          onClick={handleCopy}
          className="inline-flex items-center gap-1 text-[10px] text-muted-foreground hover:text-foreground transition-colors"
          title="Copy code"
        >
          {copied ? <Check className="size-3 text-success" /> : <Copy className="size-3" />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="overflow-x-auto p-3 text-[12px] leading-relaxed font-mono">
        {children}
      </pre>
    </div>
  );
}
