import { defineConfig } from 'orval'

/**
 * Generates the typed client from the committed OpenAPI document:
 *   - `src/api/generated/endpoints.ts` — TanStack Query hooks over fetch
 *   - `src/api/generated/model/` — request/response types
 *   - `src/api/generated/zod/` — zod schemas mirroring the server's validation
 *
 * Both the spec and the generated output are committed; CI regenerates and fails on diff.
 */
export default defineConfig({
  kvet: {
    input: '../k-vet-backend/openapi.json',
    output: {
      target: './src/api/generated/endpoints.ts',
      schemas: './src/api/generated/model',
      client: 'react-query',
      httpClient: 'fetch',
      clean: true,
      override: {
        mutator: {
          path: './src/api/fetcher.ts',
          name: 'apiFetch',
        },
        // The mutator throws `ApiError` and returns the payload itself, so the generated
        // functions must not wrap it in `{ status, data }`.
        fetch: { includeHttpResponseReturnType: false },
      },
    },
  },
  kvetZod: {
    input: '../k-vet-backend/openapi.json',
    output: {
      target: './src/api/generated/zod/schemas.ts',
      client: 'zod',
      clean: true,
    },
  },
})
