import React, { useState } from 'react';
import { Check, Copy } from 'lucide-react';

function isSafeHref(href: string): boolean {
  const trimmed = href.trim().toLowerCase();
  if (!trimmed) return false;
  if (
    trimmed.startsWith('http://') ||
    trimmed.startsWith('https://') ||
    trimmed.startsWith('mailto:') ||
    trimmed.startsWith('/') ||
    trimmed.startsWith('./') ||
    trimmed.startsWith('../') ||
    trimmed.startsWith('#')
  ) {
    return true;
  }
  return false;
}

function renderInline(text: string): React.ReactNode[] {
  const pattern = /(\[[^\]]+\]\([^)]+\)|`[^`]+`|\*\*[^*]+\*\*|~~[^~]+~~|\*[^*\n]+\*)/g;
  const tokens = text.split(pattern);
  return tokens.map((token, idx) => {
    if (!token) return null;

    if (token.startsWith('`') && token.endsWith('`') && token.length > 2) {
      return (
        <code
          key={idx}
          className="font-mono text-[13px] px-1.5 py-0.5 rounded bg-surface-soft border border-hairline text-ink"
        >
          {token.slice(1, -1)}
        </code>
      );
    }

    if (token.startsWith('**') && token.endsWith('**') && token.length > 4) {
      return (
        <strong key={idx} className="font-semibold text-ink">
          {token.slice(2, -2)}
        </strong>
      );
    }

    if (token.startsWith('~~') && token.endsWith('~~') && token.length > 4) {
      return (
        <del key={idx} className="text-muted line-through">
          {token.slice(2, -2)}
        </del>
      );
    }

    if (token.startsWith('*') && token.endsWith('*') && token.length > 2) {
      return (
        <em key={idx} className="italic text-body-strong">
          {token.slice(1, -1)}
        </em>
      );
    }

    const linkMatch = token.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
    if (linkMatch) {
      const [, label, href] = linkMatch;
      if (isSafeHref(href)) {
        return (
          <a
            key={idx}
            href={href}
            target={href.startsWith('http') ? '_blank' : undefined}
            rel={href.startsWith('http') ? 'noopener noreferrer' : undefined}
            className="underline decoration-hairline underline-offset-2 text-primary hover:text-primary-active transition-colors"
          >
            {label}
          </a>
        );
      }
      return <span key={idx}>{label}</span>;
    }

    return <React.Fragment key={idx}>{token}</React.Fragment>;
  });
}

function CodeBlock({ language, code }: { language: string; code: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard API unavailable in non-secure contexts
    }
  };

  return (
    <div className="rounded-lg border border-code-border bg-code-bg overflow-hidden">
      <div className="flex items-center justify-between px-3.5 py-1.5 border-b border-code-border bg-code-soft text-[11px] font-mono text-code-muted">
        <span>{language || 'text'}</span>
        <button
          type="button"
          onClick={handleCopy}
          aria-label="Copy code block"
          className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded hover:text-code-text hover:bg-code-elevated transition-colors cursor-pointer"
        >
          {copied ? (
            <>
              <Check className="w-3 h-3 text-success" />
              <span className="text-success">Copied</span>
            </>
          ) : (
            <>
              <Copy className="w-3 h-3" />
              <span>Copy</span>
            </>
          )}
        </button>
      </div>
      <pre className="text-code-text px-4 py-3 font-mono text-[13px] leading-relaxed overflow-x-auto dark-scroll">
        <code>{code}</code>
      </pre>
    </div>
  );
}

function isTableSeparator(line: string): boolean {
  const trimmed = line.trim();
  return /^\|?(\s*:?-+:?\s*\|)+\s*:?-+:?\s*\|?$/.test(trimmed);
}

function splitTableRow(line: string): string[] {
  const trimmed = line.trim().replace(/^\|/, '').replace(/\|$/, '');
  return trimmed.split('|').map((cell) => cell.trim());
}

function renderMarkdownTextBlock(block: string, blockKey: number) {
  const lines = block.split('\n');
  const nodes: React.ReactNode[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();

    if (!trimmed) {
      i += 1;
      continue;
    }

    if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) {
      nodes.push(<hr key={`${blockKey}-hr-${i}`} className="border-hairline my-2" />);
      i += 1;
      continue;
    }

    if (trimmed.startsWith('#### ')) {
      nodes.push(
        <h5 key={`${blockKey}-h4-${i}`} className="font-semibold text-[14px] text-ink pt-1">
          {renderInline(trimmed.slice(5))}
        </h5>
      );
      i += 1;
      continue;
    }
    if (trimmed.startsWith('### ')) {
      nodes.push(
        <h4 key={`${blockKey}-h3-${i}`} className="font-semibold text-[15px] text-ink pt-1.5">
          {renderInline(trimmed.slice(4))}
        </h4>
      );
      i += 1;
      continue;
    }
    if (trimmed.startsWith('## ')) {
      nodes.push(
        <h3 key={`${blockKey}-h2-${i}`} className="font-semibold text-base text-ink pt-2">
          {renderInline(trimmed.slice(3))}
        </h3>
      );
      i += 1;
      continue;
    }
    if (trimmed.startsWith('# ')) {
      nodes.push(
        <h2 key={`${blockKey}-h1-${i}`} className="font-semibold text-lg text-ink pt-2">
          {renderInline(trimmed.slice(2))}
        </h2>
      );
      i += 1;
      continue;
    }

    // GFM Table
    if (trimmed.includes('|') && i + 1 < lines.length && isTableSeparator(lines[i + 1])) {
      const headers = splitTableRow(trimmed);
      const rows: string[][] = [];
      let j = i + 2;
      while (j < lines.length && lines[j].trim().includes('|')) {
        rows.push(splitTableRow(lines[j]));
        j += 1;
      }
      nodes.push(
        <div key={`${blockKey}-tbl-${i}`} className="overflow-x-auto rounded-lg border border-hairline">
          <table className="w-full text-left border-collapse text-[13px]">
            <thead className="bg-surface-soft border-b border-hairline">
              <tr>
                {headers.map((h, hIdx) => (
                  <th key={hIdx} className="px-3 py-2 font-medium text-ink whitespace-nowrap">
                    {renderInline(h)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-hairline-soft">
              {rows.map((row, rIdx) => (
                <tr key={rIdx} className="hover:bg-surface-soft/50">
                  {headers.map((_, cIdx) => (
                    <td key={cIdx} className="px-3 py-2 text-body-strong">
                      {renderInline(row[cIdx] ?? '')}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
      i = j;
      continue;
    }

    // Blockquote
    if (trimmed.startsWith('> ')) {
      const quoteLines: string[] = [];
      while (i < lines.length && lines[i].trim().startsWith('> ')) {
        quoteLines.push(lines[i].trim().slice(2));
        i += 1;
      }
      nodes.push(
        <blockquote
          key={`${blockKey}-bq-${i}`}
          className="border-l-2 border-primary/60 pl-3.5 py-0.5 text-muted space-y-1 italic"
        >
          {quoteLines.map((q, qIdx) => (
            <p key={qIdx}>{renderInline(q)}</p>
          ))}
        </blockquote>
      );
      continue;
    }

    // Unordered / task list
    if (/^[-*]\s+/.test(trimmed)) {
      const items: { checked: boolean | null; text: string }[] = [];
      while (i < lines.length && /^[-*]\s+/.test(lines[i].trim())) {
        const rawItem = lines[i].trim().slice(2).trim();
        if (rawItem.startsWith('[x] ') || rawItem.startsWith('[X] ')) {
          items.push({ checked: true, text: rawItem.slice(4) });
        } else if (rawItem.startsWith('[ ] ')) {
          items.push({ checked: false, text: rawItem.slice(4) });
        } else {
          items.push({ checked: null, text: rawItem });
        }
        i += 1;
      }
      nodes.push(
        <ul key={`${blockKey}-ul-${i}`} className="space-y-1 pl-1">
          {items.map((item, idx) => (
            <li key={idx} className="flex items-start gap-2">
              {item.checked === null ? (
                <span className="text-muted select-none">•</span>
              ) : (
                <input
                  type="checkbox"
                  readOnly
                  checked={item.checked}
                  aria-label="Task item"
                  className="mt-1 accent-primary"
                />
              )}
              <span className="min-w-0 flex-1">{renderInline(item.text)}</span>
            </li>
          ))}
        </ul>
      );
      continue;
    }

    // Ordered list
    if (/^\d+\.\s+/.test(trimmed)) {
      const items: string[] = [];
      while (i < lines.length && /^\d+\.\s+/.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(/^\d+\.\s+/, ''));
        i += 1;
      }
      nodes.push(
        <ol key={`${blockKey}-ol-${i}`} className="list-decimal pl-5 space-y-1">
          {items.map((item, idx) => (
            <li key={idx}>{renderInline(item)}</li>
          ))}
        </ol>
      );
      continue;
    }

    nodes.push(
      <p key={`${blockKey}-p-${i}`} className="whitespace-pre-wrap break-words">
        {renderInline(line)}
      </p>
    );
    i += 1;
  }

  return (
    <div key={blockKey} className="space-y-1.5">
      {nodes}
    </div>
  );
}

export function ProseContent({
  content,
  compact = false,
}: {
  content: string;
  compact?: boolean;
}) {
  const blocks = content.split(/(```[\s\S]*?```)/g);

  return (
    <div
      className={
        compact
          ? 'space-y-2 text-[14px] text-body-strong leading-[1.52]'
          : 'space-y-2.5 text-[15px] text-body-strong leading-[1.6]'
      }
    >
      {blocks.map((block, bIdx) => {
        if (block.startsWith('```') && block.endsWith('```')) {
          const withoutFence = block.slice(3, -3);
          const firstNewline = withoutFence.indexOf('\n');
          const langMatch =
            firstNewline >= 0
              ? withoutFence.slice(0, firstNewline).trim()
              : '';
          const code =
            firstNewline >= 0
              ? withoutFence.slice(firstNewline + 1).replace(/\n$/, '')
              : withoutFence;
          return <CodeBlock key={bIdx} language={langMatch} code={code} />;
        }

        return renderMarkdownTextBlock(block, bIdx);
      })}
    </div>
  );
}
