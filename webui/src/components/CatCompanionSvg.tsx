import React from 'react';
import type { CompanionConfig, CompanionMood, CompanionReaction } from '../types/cockpit';

interface CatCompanionSvgProps {
  companion: CompanionConfig;
  mood: CompanionMood;
  reaction?: CompanionReaction;
  size?: 'sm' | 'md' | 'lg';
}

export function CatCompanionSvg({
  companion,
  mood,
  reaction = null,
  size = 'md',
}: CatCompanionSvgProps) {
  const dimensions =
    size === 'sm' ? 'w-9 h-9' : size === 'lg' ? 'w-28 h-28' : 'w-20 h-20';

  const isWorking = mood === 'thinking';
  const isApproval = mood === 'approval';
  const isCelebrating = reaction === 'celebrate' || reaction === 'petted';
  const isExcited = mood === 'excited' || isCelebrating;

  return (
    <svg
      viewBox="0 0 96 96"
      fill="none"
      aria-hidden="true"
      className={`${dimensions} select-none overflow-visible`}
    >
      {/* Subtle ground shadow */}
      <ellipse cx="48" cy="84" rx="26" ry="4" fill="var(--pc-ink)" fillOpacity="0.08" />

      {/* Animated Tail */}
      <path
        d="M28 70C15 69 10 56 16 46C18 42 22 44 21 48C18 55 21 63 30 64"
        stroke={companion.pattern === 'siamese' ? companion.furSecondary : companion.furPrimary}
        strokeWidth="6.5"
        strokeLinecap="round"
        className={isWorking || isExcited ? 'animate-cat-tail' : 'animate-cat-tail-slow'}
      />

      {/* Breathing / Celebration Body + Head Group */}
      <g className={isCelebrating ? 'animate-cat-celebrate' : 'animate-cat-breathe'}>
        {/* Torso */}
        <path
          d="M30 78C30 62 36 53 48 53C60 53 66 62 66 78C66 80 64 82 61 82H35C32 82 30 80 30 78Z"
          fill={companion.furPrimary}
        />

        {/* Pattern Chest Bib */}
        {(companion.pattern === 'tuxedo' ||
          companion.pattern === 'calico' ||
          companion.pattern === 'tabby') && (
          <path
            d="M41 56C44 55 52 55 55 56L53 82H43L41 56Z"
            fill={companion.furSecondary}
          />
        )}

        {/* Scarf Accessory */}
        {companion.accessory === 'scarf' && (
          <g>
            <path
              d="M34 54C40 58 56 58 62 54L60 61C54 64 42 64 36 61L34 54Z"
              fill="var(--pc-primary)"
            />
            <path
              d="M52 59L57 72L49 71L47 60Z"
              fill="var(--pc-primary-active)"
            />
          </g>
        )}

        {/* Left Ear */}
        <g className={isWorking ? 'animate-cat-ear' : ''}>
          <path
            d="M28 34L27 15L41 25Z"
            fill={companion.pattern === 'siamese' ? companion.furSecondary : companion.furPrimary}
          />
          <path d="M30 31L29.5 19.5L38 25.5Z" fill="var(--pc-primary)" fillOpacity="0.45" />
        </g>

        {/* Right Ear */}
        <g className={isWorking ? 'animate-cat-ear' : ''}>
          <path
            d="M68 34L69 15L55 25Z"
            fill={companion.pattern === 'siamese' ? companion.furSecondary : companion.furPrimary}
          />
          <path d="M66 31L66.5 19.5L58 25.5Z" fill="var(--pc-primary)" fillOpacity="0.45" />
        </g>

        {/* Head Base */}
        <ellipse cx="48" cy="40" rx="22" ry="18" fill={companion.furPrimary} />

        {/* Pattern Markings */}
        {companion.pattern === 'tuxedo' && (
          <path
            d="M33 43C38 40 43 45 48 45C53 45 58 40 63 43C62 52 55 57 48 57C41 57 34 52 33 43Z"
            fill={companion.furSecondary}
          />
        )}

        {companion.pattern === 'siamese' && (
          <ellipse
            cx="48"
            cy="43"
            rx="12"
            ry="9.5"
            fill={companion.furSecondary}
            fillOpacity="0.92"
          />
        )}

        {companion.pattern === 'calico' && (
          <>
            <path d="M29 34C34 27 42 25 47 28C43 34 36 37 29 34Z" fill={companion.furSecondary} />
            <circle cx="59" cy="34" r="7" fill="var(--pc-ink)" fillOpacity="0.72" />
          </>
        )}

        {companion.pattern === 'tabby' && (
          <g stroke={companion.furSecondary} strokeWidth="2" strokeLinecap="round" opacity="0.85">
            <path d="M44 25V30" />
            <path d="M48 24V31" />
            <path d="M52 25V30" />
          </g>
        )}

        {/* Eyes */}
        <g className="animate-cat-blink">
          {isExcited ? (
            <>
              <path
                d="M36 40C38 36 42 36 44 40"
                stroke={companion.eyeColor}
                strokeWidth="2.4"
                strokeLinecap="round"
              />
              <path
                d="M52 40C54 36 58 36 60 40"
                stroke={companion.eyeColor}
                strokeWidth="2.4"
                strokeLinecap="round"
              />
            </>
          ) : (
            <>
              <circle cx="39.5" cy="39.5" r={isApproval ? 4 : 3.6} fill={companion.eyeColor} />
              <circle cx="56.5" cy="39.5" r={isApproval ? 4 : 3.6} fill={companion.eyeColor} />
              <circle cx="39.5" cy="39.5" r="1.6" fill="#141413" />
              <circle cx="56.5" cy="39.5" r="1.6" fill="#141413" />
              <circle cx="40.6" cy="38.2" r="0.9" fill="#ffffff" />
              <circle cx="57.6" cy="38.2" r="0.9" fill="#ffffff" />
            </>
          )}
        </g>

        {/* Nose & Mouth */}
        <path d="M46.2 44.5H49.8L48 46.8Z" fill="var(--pc-primary)" />
        <path
          d="M44.5 48.2C46 49.5 47.5 49 48 47.4C48.5 49 50 49.5 51.5 48.2"
          stroke={companion.pattern === 'tuxedo' ? '#141413' : companion.furSecondary}
          strokeWidth="1.4"
          strokeLinecap="round"
          strokeLinejoin="round"
        />

        {/* Whiskers */}
        <g stroke={companion.furSecondary} strokeOpacity="0.7" strokeWidth="1.2" strokeLinecap="round">
          <path d="M22 41L30 42.5M21 45.5L30 45.5" />
          <path d="M74 41L66 42.5M75 45.5L66 45.5" />
        </g>

        {/* Glasses Accessory */}
        {companion.accessory === 'glasses' && (
          <g stroke="var(--pc-ink)" strokeWidth="1.7">
            <circle cx="39.5" cy="39.5" r="6.2" fill="var(--pc-canvas)" fillOpacity="0.18" />
            <circle cx="56.5" cy="39.5" r="6.2" fill="var(--pc-canvas)" fillOpacity="0.18" />
            <path d="M45.7 39.5H50.3" />
          </g>
        )}

        {/* Headphones Accessory */}
        {companion.accessory === 'headphones' && (
          <g>
            <path
              d="M25 38C25 22 35 15 48 15C61 15 71 22 71 38"
              stroke="var(--pc-code-bg)"
              strokeWidth="3.2"
              strokeLinecap="round"
            />
            <rect x="21" y="33" width="6" height="12" rx="3" fill="var(--pc-primary)" />
            <rect x="69" y="33" width="6" height="12" rx="3" fill="var(--pc-primary)" />
          </g>
        )}

        {/* Front Paws */}
        <ellipse cx="40" cy="80" rx="5" ry="3.4" fill={companion.furSecondary} />
        <ellipse cx="56" cy="80" rx="5" ry="3.4" fill={companion.furSecondary} />
      </g>

      {/* Thinking / Streaming Activity Indicator */}
      {isWorking && (
        <g transform="translate(69, 10)">
          <rect
            x="0"
            y="0"
            width="22"
            height="14"
            rx="4"
            fill="var(--pc-code-bg)"
            stroke="var(--pc-primary)"
            strokeWidth="1.2"
          />
          <circle cx="6" cy="7" r="1.5" fill="var(--pc-primary)" className="animate-paw-step" />
          <circle
            cx="11"
            cy="7"
            r="1.5"
            fill="var(--pc-primary)"
            className="animate-paw-step"
            style={{ animationDelay: '150ms' }}
          />
          <circle
            cx="16"
            cy="7"
            r="1.5"
            fill="var(--pc-primary)"
            className="animate-paw-step"
            style={{ animationDelay: '300ms' }}
          />
        </g>
      )}

      {/* Expressive Celebration / Petted Sparkles */}
      {isCelebrating && (
        <g aria-hidden="true">
          <path
            d="M18 20L19.5 16L21 20L25 21.5L21 23L19.5 27L18 23L14 21.5Z"
            fill="var(--pc-primary)"
            className="animate-sparkle-pop"
          />
          <path
            d="M75 18L76.3 14.5L77.6 18L81 19.3L77.6 20.6L76.3 24L75 20.6L71.5 19.3Z"
            fill="var(--pc-accent-amber)"
            className="animate-sparkle-pop"
            style={{ animationDelay: '240ms' }}
          />
          {reaction === 'petted' && (
            <path
              d="M48 11C48 9 45.5 7.5 44 9C42.5 10.5 44.5 13 48 15.5C51.5 13 53.5 10.5 52 9C50.5 7.5 48 9 48 11Z"
              fill="var(--pc-primary)"
              className="animate-sparkle-pop"
              style={{ animationDelay: '120ms' }}
            />
          )}
        </g>
      )}
    </svg>
  );
}
