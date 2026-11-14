export class ApiError extends Error {
  public readonly status: number;
  public readonly code: string;
  public readonly data?: Record<string, unknown>;

  constructor(
    message: string,
    status: number,
    code = 'API_ERROR',
    data?: Record<string, unknown>
  ) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.data = data;
  }
}

export async function requestJson<T>(
  url: string,
  options?: RequestInit
): Promise<T> {
  const response = await fetch(url, {
    ...options,
    headers: {
      ...(options?.body ? { 'Content-Type': 'application/json' } : {}),
      ...(options?.headers ?? {}),
    },
  });

  let parsed: Record<string, unknown> = {};
  try {
    parsed = (await response.json()) as Record<string, unknown>;
  } catch {
    // Non-JSON error body fallback
  }

  if (!response.ok) {
    const message =
      typeof parsed.error === 'string'
        ? parsed.error
        : `Request failed with status ${response.status}`;
    const code = typeof parsed.code === 'string' ? parsed.code : 'HTTP_ERROR';
    throw new ApiError(message, response.status, code, parsed);
  }

  return parsed as T;
}
