import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      "dist",
      "node_modules",
      "target",
      "src-tauri/target",
      "src-tauri/gen",
      "eslint.config.js",
      "**/._*",
      "**/.DS_Store",
    ],
  },
  ...tseslint.configs.strictTypeChecked,
  {
    ...tseslint.configs.disableTypeChecked,
    files: ["src/youtube/bridge.js", "extension/**/*.js"],
    languageOptions: {
      globals: {
        chrome: "readonly",
        console: "readonly",
        crypto: "readonly",
        document: "readonly",
        atob: "readonly",
        btoa: "readonly",
        TextEncoder: "readonly",
        URL: "readonly",
        window: "readonly",
      },
    },
  },
  {
    files: ["src/**/*.ts", "src/**/*.tsx", "vite.config.ts"],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      "react-hooks": reactHooks,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
    },
  },
);
