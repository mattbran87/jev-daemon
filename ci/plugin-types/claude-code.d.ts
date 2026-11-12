// Stands in for the module that Claude Code writes beside a mod it loads
// (.claude-plugin/types/claude-code/index.d.ts). The plugin's index.d.ts adds
// `jev` to EngineInterface, so the check needs the interface to exist.
declare module 'claude-code' {
  interface EngineInterface {}
}
