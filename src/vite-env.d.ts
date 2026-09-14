/// <reference types="vite/client" />

/// Build-time environment — values are inlined by Vite at build time.
/// VITE_TMDB_TOKEN: the shared TMDB key (v3 API key or v4 read token),
/// from `.env` (local builds) or the CI secret of the same name. Never
/// committed — see .env.example.
interface ImportMetaEnv {
  readonly VITE_TMDB_TOKEN?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

declare module "*.png" {
  const src: string;
  export default src;
}
