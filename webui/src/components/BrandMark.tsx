import React from 'react';

/**
 * PurrCode Option A: Geometric Cat + Code Bracket Mark
 * Clean angular cat head with pointed ears and code-bracket whisker accents.
 */
export function BrandMark({ className = 'w-6 h-6' }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 28 28"
      fill="none"
      aria-hidden="true"
      className={className}
    >
      {/* Geometric cat head silhouette with pointed ears */}
      <path
        d="M5 10.5L6.5 4.2L11.2 8.2H16.8L21.5 4.2L23 10.5C23.8 12.2 24 14.3 23.6 16.4C22.8 20.6 19.1 23.5 14 23.5C8.9 23.5 5.2 20.6 4.4 16.4C4 14.3 4.2 12.2 5 10.5Z"
        stroke="var(--pc-primary)"
        strokeWidth="2"
        strokeLinejoin="round"
      />
      {/* Code bracket eyes / angular visor < > */}
      <path
        d="M10.4 12.6L8.3 14.3L10.4 16"
        stroke="var(--pc-ink)"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M17.6 12.6L19.7 14.3L17.6 16"
        stroke="var(--pc-ink)"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      {/* Minimalist nose & whisker slash */}
      <path
        d="M13.1 17.5H14.9L14 18.7L13.1 17.5Z"
        fill="var(--pc-primary)"
      />
      <path
        d="M2.2 14.2H4.2M2.5 16.6H4.5M23.8 14.2H25.8M23.5 16.6H25.5"
        stroke="var(--pc-primary)"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}
