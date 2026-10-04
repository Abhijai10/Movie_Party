/// <reference types="vite/client" />

/// Build-time environment — values are inlined by Vite at build time.
/// VITE_TMDB_TOKEN: the shared TMDB key (v3 API key or v4 read token),
/// from `.env` (local builds) or the CI secret of the same name. Never
/// committed — see .env.example.
///
/// NOT A SECRET IN THE SHIPPED APP. Vite inlines `VITE_*` values into the
/// client bundle, so this value is recoverable from any built installer.
/// Treat it as a public, rotatable client credential: it must be safe to
/// expose. Anything that genuinely must stay secret cannot be shipped this
/// way. See docs/RELEASE_PROCESS.md § "The TMDB credential".
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
