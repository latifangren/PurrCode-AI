import type { NextFunction, Request, Response } from 'express';

export interface ApiErrorEnvelope {
  error: string;
  code: string;
  details?: string;
}

export function sendError(
  res: Response,
  status: number,
  code: string,
  error: string,
  details?: string
) {
  const body: ApiErrorEnvelope = { error, code };
  if (details) body.details = details;
  res.status(status).json(body);
}

export function securityHeadersMiddleware(
  _req: Request,
  res: Response,
  next: NextFunction
) {
  res.setHeader('X-Content-Type-Options', 'nosniff');
  res.setHeader('X-Frame-Options', 'SAMEORIGIN');
  res.setHeader('Referrer-Policy', 'no-referrer');
  next();
}

export function parseNonEmptyString(val: unknown): string | null {
  if (typeof val !== 'string') return null;
  const trimmed = val.trim();
  return trimmed.length > 0 ? trimmed : null;
}
