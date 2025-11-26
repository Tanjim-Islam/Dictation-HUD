module.exports = {
  root: true,
  env: { browser: true, es2022: true, node: true },
  parserOptions: { ecmaVersion: 'latest', sourceType: 'module' },
  extends: ['eslint:recommended', 'plugin:react-hooks/recommended', 'prettier'],
  settings: { react: { version: 'detect' } },
  ignorePatterns: ['dist', 'node_modules', 'src-tauri/target'],
  overrides: [
    { files: ['**/*.{ts,tsx}'], parser: '@typescript-eslint/parser', plugins: ['@typescript-eslint'], extends: ['plugin:@typescript-eslint/recommended'] }
  ]
};

