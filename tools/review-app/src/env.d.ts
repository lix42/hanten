/// <reference types="vite/client" />

/**
 * StyleX's dev-only virtual modules. The unplugin serves these from the dev
 * server; neither exists in a build, which is why `StyleXStyles` imports the
 * runtime one behind `import.meta.env.DEV`.
 */
declare module "virtual:stylex:runtime";
