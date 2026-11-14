import React from 'react';

function renderInline(text: string): React.ReactNode[] {
  const tokens = text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g);
  return tokens.map((token, idx) => {
    if (token.startsWith('**') && token.endsWith('**') && token.length > 4) {
      return (
        <strong key={idx} className="font-semibold text-ink">
          {token.slice(2, -2)}
        </strong>
      );
    }
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
    return <React.Fragment key={idx}>{token}</React.Fragment>;
  });
}

export function ProseContent({ content }: { content: string }) {
  const blocks = content.split(/(```[\s\S]*?```)/g);

  return (
    <div className="space-y-2.5 text-[15px] text-body-strong leading-[1.6]">
      {blocks.map((block, bIdx) => {
        if (block.startsWith('```') && block.endsWith('```')) {
          const inner = block.slice(3, -3).replace(/^[a-zA-Z0-9_-]+\n/, '');
          return (
            <pre
              key={bIdx}
              className="rounded-lg bg-code-bg text-code-text p-4 font-mono text-[13px] leading-relaxed overflow-x-auto dark-scroll"
            >
              <code>{inner}</code>
            </pre>
          );
        }

        const lines = block.split('\n');
        return (
          <div key={bIdx} className="space-y-1.5">
            {lines.map((line, lIdx) => {
              const trimmed = line.trim();
              if (!trimmed) return null;
              if (trimmed.startsWith('### ')) {
                return (
                  <h4 key={lIdx} className="font-medium text-ink pt-1">
                    {renderInline(trimmed.slice(4))}
                  </h4>
                );
              }
              if (trimmed.startsWith('## ')) {
                return (
                  <h3 key={lIdx} className="font-editorial text-xl text-ink pt-1">
                    {renderInline(trimmed.slice(3))}
                  </h3>
                );
              }
              if (trimmed.startsWith('- ') || trimmed.startsWith('* ')) {
                return (
                  <p key={lIdx} className="pl-4 relative">
                    <span className="absolute left-0 text-primary" aria-hidden="true">
                      ·
                    </span>
                    {renderInline(trimmed.slice(2))}
                  </p>
                );
              }
              return <p key={lIdx}>{renderInline(line)}</p>;
            })}
          </div>
        );
      })}
    </div>
  );
}
