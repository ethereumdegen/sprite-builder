// ESLint flat config (enforcement; see ../enforcement/README.md).
// Encodes the frontend cross-cutting ADRs as lint rules. Requires:
//   npm i -D eslint typescript-eslint @eslint/js eslint-plugin-react-hooks
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";

export default tseslint.config(
  { ignores: ["dist/**", "node_modules/**"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,

      // ADR 0007 — frontend state lives in per-domain Zustand stores.
      // Redux and React Context-as-state-store are disallowed.
      "no-restricted-imports": [
        "error",
        {
          paths: [
            { name: "redux", message: "ADR 0007: use a per-domain Zustand store, not Redux." },
            { name: "react-redux", message: "ADR 0007: use a per-domain Zustand store, not Redux." },
            { name: "@reduxjs/toolkit", message: "ADR 0007: use a per-domain Zustand store, not Redux." },
          ],
        },
      ],
      "no-restricted-syntax": [
        "error",
        {
          selector: "CallExpression[callee.name='createContext']",
          message: "ADR 0007: app state belongs in a Zustand store, not React Context.",
        },
      ],
    },
  },
  {
    // ADR 0008 — exactly one typed API client. Components must not call fetch
    // directly; all backend access goes through src/api.ts.
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/api.ts"],
    rules: {
      "no-restricted-globals": [
        "error",
        { name: "fetch", message: "ADR 0008: call the typed client in src/api.ts, not fetch directly." },
      ],
    },
  }
);
